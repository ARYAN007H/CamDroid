// Pure domain logic for CamDroid Studio (Apple-grade Continuity Camera & Native Desktop App)
// Decoupled from DOM/React/Tauri for deterministic unit testing.

export type FilterId =
  | 'original'
  | 'studio_natural'
  | 'dramatic'
  | 'dramatic_warm'
  | 'dramatic_cool'
  | 'silvertone'
  | 'noir'
  | 'cyber_thermal'
  | 'vintage_film';

export type CameraFilterId = FilterId;

export interface CameraFilterSpec {
  id: FilterId;
  name: string;
  shortLabel: string;
  badge: string;
  description: string;
  contrast: number;
  saturate: number;
  brightness: number;
  sepia: number;
  hueRotateDeg: number;
  grayscale: number;
  tintRgba: string;
  vignetteStrength: number;
  cssFilter: string;
}

export const CAMERA_FILTERS: readonly CameraFilterSpec[] = [
  {
    id: 'original',
    name: 'Original',
    shortLabel: 'RAW',
    badge: 'RAW',
    description: 'True 10-bit sensor color pipeline',
    contrast: 1.0,
    saturate: 1.0,
    brightness: 1.0,
    sepia: 0,
    hueRotateDeg: 0,
    grayscale: 0,
    tintRgba: 'rgba(0,0,0,0)',
    vignetteStrength: 0.1,
    cssFilter: 'none',
  },
  {
    id: 'studio_natural',
    name: 'Studio Clean',
    shortLabel: 'STD',
    badge: 'STD',
    description: 'Apple Continuity skin tone balance & highlight roll-off',
    contrast: 1.05,
    saturate: 1.08,
    brightness: 1.05,
    sepia: 0.03,
    hueRotateDeg: -2,
    grayscale: 0,
    tintRgba: 'rgba(255, 224, 195, 0.05)',
    vignetteStrength: 0.14,
    cssFilter: 'brightness(1.05) contrast(1.05) saturate(1.08)',
  },
  {
    id: 'dramatic',
    name: 'Dramatic',
    shortLabel: 'DRM',
    badge: 'DRM',
    description: 'Deep shadow density with crisp micro-contrast',
    contrast: 1.28,
    saturate: 0.86,
    brightness: 0.96,
    sepia: 0,
    hueRotateDeg: 0,
    grayscale: 0,
    tintRgba: 'rgba(15, 23, 42, 0.12)',
    vignetteStrength: 0.36,
    cssFilter: 'brightness(0.96) contrast(1.28) saturate(0.86)',
  },
  {
    id: 'dramatic_warm',
    name: 'Dramatic Warm',
    shortLabel: 'DRW',
    badge: 'DRW',
    description: 'Golden-hour tungsten warmth with rich shadow contrast',
    contrast: 1.22,
    saturate: 1.14,
    brightness: 1.02,
    sepia: 0.2,
    hueRotateDeg: -6,
    grayscale: 0,
    tintRgba: 'rgba(251, 146, 60, 0.12)',
    vignetteStrength: 0.32,
    cssFilter: 'brightness(1.02) contrast(1.22) saturate(1.14) sepia(0.2)',
  },
  {
    id: 'dramatic_cool',
    name: 'Dramatic Cool',
    shortLabel: 'DRC',
    badge: 'DRC',
    description: 'Anamorphic cyan-and-steel cinema grade',
    contrast: 1.24,
    saturate: 0.92,
    brightness: 0.98,
    sepia: 0.04,
    hueRotateDeg: 185,
    grayscale: 0,
    tintRgba: 'rgba(56, 189, 248, 0.12)',
    vignetteStrength: 0.34,
    cssFilter: 'brightness(0.98) contrast(1.24) saturate(0.92) hue-rotate(-12deg)',
  },
  {
    id: 'silvertone',
    name: 'Silvertone',
    shortLabel: 'SLV',
    badge: 'SLV',
    description: 'Luminance-weighted metallic monochrome with silver midtones',
    contrast: 1.2,
    saturate: 0.0,
    brightness: 1.06,
    sepia: 0.12,
    hueRotateDeg: 0,
    grayscale: 0.92,
    tintRgba: 'rgba(226, 232, 240, 0.08)',
    vignetteStrength: 0.3,
    cssFilter: 'brightness(1.06) contrast(1.2) grayscale(0.92) sepia(0.12)',
  },
  {
    id: 'noir',
    name: 'Mono Noir',
    shortLabel: 'NOR',
    badge: 'NOR',
    description: 'High-contrast panchromatic black & white film look',
    contrast: 1.46,
    saturate: 0.0,
    brightness: 0.94,
    sepia: 0,
    hueRotateDeg: 0,
    grayscale: 1.0,
    tintRgba: 'rgba(0, 0, 0, 0.16)',
    vignetteStrength: 0.46,
    cssFilter: 'brightness(0.94) contrast(1.46) grayscale(1)',
  },
  {
    id: 'vintage_film',
    name: 'Kodachrome',
    shortLabel: 'KDC',
    badge: 'KDC',
    description: 'Warm analog halation with lifted matte blacks',
    contrast: 1.12,
    saturate: 1.18,
    brightness: 1.03,
    sepia: 0.26,
    hueRotateDeg: -5,
    grayscale: 0,
    tintRgba: 'rgba(245, 158, 11, 0.09)',
    vignetteStrength: 0.28,
    cssFilter: 'brightness(1.03) contrast(1.12) saturate(1.18) sepia(0.26)',
  },
  {
    id: 'cyber_thermal',
    name: 'Neon Halation',
    shortLabel: 'NEO',
    badge: 'NEO',
    description: 'Bi-color studio halation & vibrant specular grade',
    contrast: 1.3,
    saturate: 1.55,
    brightness: 1.03,
    sepia: 0,
    hueRotateDeg: -24,
    grayscale: 0,
    tintRgba: 'rgba(168, 85, 247, 0.12)',
    vignetteStrength: 0.28,
    cssFilter: 'brightness(1.03) contrast(1.3) saturate(1.55) hue-rotate(-24deg)',
  },
] as const;

export interface OpticalLensPreset {
  id: string;
  label: string;
  zoom: number;
  focalLengthMm: string;
  aperture: string;
  role: string;
}

export const OPTICAL_LENSES: readonly OpticalLensPreset[] = [
  {
    id: 'ultrawide',
    label: '0.5x',
    zoom: 0.5,
    focalLengthMm: '13mm',
    aperture: 'f/2.2',
    role: 'Ultra-Wide / Desk View',
  },
  {
    id: 'main',
    label: '1x',
    zoom: 1.0,
    focalLengthMm: '24mm',
    aperture: 'f/1.68',
    role: 'Main Wide Sensor',
  },
  {
    id: 'tele2x',
    label: '2x',
    zoom: 2.0,
    focalLengthMm: '48mm',
    aperture: 'f/1.68',
    role: 'Optical Quality Crop',
  },
  {
    id: 'tele5x',
    label: '5x',
    zoom: 5.0,
    focalLengthMm: '120mm',
    aperture: 'f/2.8',
    role: 'Periscope Telephoto',
  },
] as const;

export interface RingLightConfig {
  enabled: boolean;
  kelvin: number; // 2700K (Warm) to 6500K (Cool Daylight)
  intensity: number; // 0.15..1.00 or 15..100
  widthPx: number; // 16 to 72
}

export interface StudioEffectsConfig {
  portraitBokeh: boolean;
  portraitApertureFStop: number; // 1.4 to 16
  studioLight: boolean;
  studioLightIntensity: number; // 0.2 to 1.0
  centerStage: boolean;
  deskView: boolean;
  edgeLight: RingLightConfig;
}

export interface CameraHardwareState {
  lens: 'back' | 'front';
  zoom: 0.5 | 1 | 2 | 5;
  ev: number;
  iso: 'AUTO' | 100 | 200 | 400 | 800 | 1600;
  shutter: string;
  wb: number;
  torch: boolean;
  afMode: 'continuous' | 'manual';
  resolution: '720p' | '1080p' | '4K';
  fps: 30 | 60;
}

export interface BackendUpgradeStatus {
  id: number;
  title: string;
  subsystem: string;
  metric: string;
}

export const BACKEND_UPGRADES: readonly BackendUpgradeStatus[] = [
  {
    id: 1,
    title: 'Zero-Copy GPU Surface Pipeline',
    subsystem: 'VideoEncoder.kt',
    metric: 'COLOR_FormatSurface · 0 CPU copies',
  },
  {
    id: 2,
    title: '14-Byte v2.0 Wire Header & PTS Sync',
    subsystem: 'ProtocolHandler.kt / wire.rs',
    metric: '64-bit µs PTS + Flags',
  },
  {
    id: 3,
    title: 'Instant IDR Keyframe Recovery',
    subsystem: 'VideoEncoder / remote.rs',
    metric: 'request_keyframe < 16ms',
  },
  {
    id: 4,
    title: 'SIMD libswscale YUV420P Fast Path',
    subsystem: 'v4l2_output.rs',
    metric: 'SWS_FAST_BILINEAR SIMD',
  },
  {
    id: 5,
    title: 'Automatic USB ADB + Wi-Fi Failover',
    subsystem: 'connection/mod.rs',
    metric: 'Seamless 4747 Handover',
  },
  {
    id: 6,
    title: 'Lock-Free Bounded SPSC Ring Queue',
    subsystem: 'StreamServer.kt',
    metric: 'Drops stale P-frames on spike',
  },
  {
    id: 7,
    title: 'Dynamic Adaptive Bitrate (ABR)',
    subsystem: 'StreamServer / VideoEncoder',
    metric: 'PARAMETER_KEY_VIDEO_BITRATE',
  },
  {
    id: 8,
    title: 'Shared Desktop Preview + Virtual Cam',
    subsystem: 'desktop/src/lib.rs',
    metric: 'Dual Tauri UI + OBS Sink',
  },
  {
    id: 9,
    title: 'Camera2 Tap-to-Focus & Manual Pro ISP',
    subsystem: 'CameraManager.kt',
    metric: 'MeteringRectangle AF/AE + ISO',
  },
  {
    id: 10,
    title: 'Persistent Watchdog & Thermal Guard',
    subsystem: 'CameraStreamingService.kt',
    metric: 'Auto-restart + WakeLock',
  },
] as const;

export function kelvinToLabel(kelvin: number): string {
  if (kelvin <= 3400) return 'Warm Tungsten';
  if (kelvin >= 5400) return 'Cool Daylight';
  return 'Studio Neutral';
}

/**
 * Converts a color temperature in Kelvin (2700K - 6500K) to an RGB tuple [r, g, b].
 */
export function kelvinToRgb(kelvin: number): { r: number; g: number; b: number } {
  const clamped = Math.max(2000, Math.min(10000, kelvin));
  const temp = clamped / 100;

  let r: number;
  let g: number;
  let b: number;

  if (temp <= 66) {
    r = 255;
    g = 99.4708025861 * Math.log(temp) - 161.1195681661;
    if (temp <= 19) {
      b = 0;
    } else {
      b = 138.5177312231 * Math.log(temp - 10) - 305.0447927307;
    }
  } else {
    r = 329.698727446 * Math.pow(temp - 60, -0.1332047592);
    g = 288.1221695283 * Math.pow(temp - 60, -0.0755148492);
    b = 255;
  }

  return {
    r: Math.round(Math.max(0, Math.min(255, r))),
    g: Math.round(Math.max(0, Math.min(255, g))),
    b: Math.round(Math.max(0, Math.min(255, b))),
  };
}

/**
 * Computes the physical & optical illumination parameters for the Virtual Ring Light (Edge Light).
 * Supports intensity passed as either normalized [0..1] or percentage [1..100].
 */
export function computeRingLightStyle(config: RingLightConfig): {
  rgbString: string;
  borderColor: string;
  glowColor: string;
  glowSpreadPx: number;
  borderAlpha: number;
  subjectFillBoost: number;
  brightnessMultiplier: number;
  displayBrightnessPercent: number;
  sensorEvGain: number;
  kelvinLabel: string;
} {
  const { r, g, b } = kelvinToRgb(config.kelvin);
  const normalizedIntensity =
    config.intensity > 1.0 ? config.intensity / 100 : config.intensity;
  const clampedIntensity = Math.max(0, Math.min(1, normalizedIntensity));
  const widthPx = config.widthPx ?? 38;
  const glowSpreadPx = Math.round(widthPx * (1.2 + clampedIntensity * 1.4));
  const borderAlpha = config.enabled ? Number((0.65 + clampedIntensity * 0.35).toFixed(2)) : 0;

  // Functional brightness boost whenever Ring Light is ON:
  const subjectFillBoost = config.enabled ? Number((0.18 + clampedIntensity * 0.42).toFixed(3)) : 0;
  const brightnessMultiplier = config.enabled
    ? Number((1.16 + clampedIntensity * 0.32).toFixed(3))
    : 1.0;
  const displayBrightnessPercent = config.enabled
    ? Math.min(100, Math.round(72 + clampedIntensity * 28))
    : 65;
  const sensorEvGain = config.enabled ? Number((0.6 + clampedIntensity * 1.2).toFixed(1)) : 0.0;

  const kelvinLabel = kelvinToLabel(config.kelvin);

  return {
    rgbString: `${r}, ${g}, ${b}`,
    borderColor: `rgba(${r}, ${g}, ${b}, ${borderAlpha})`,
    glowColor: `rgba(${r}, ${g}, ${b}, ${(borderAlpha * 0.55).toFixed(2)})`,
    glowSpreadPx,
    borderAlpha,
    subjectFillBoost,
    brightnessMultiplier,
    displayBrightnessPercent,
    sensorEvGain,
    kelvinLabel,
  };
}

/**
 * Builds the composite CSS filter string for the camera viewfinder given the active
 * filter preset, exposure compensation EV (-4..+4), Studio Light, and Virtual Ring Light.
 */
export function buildCanvasFilterString(params: {
  filterId: FilterId;
  exposureEv: number;
  studioLight: boolean;
  studioLightIntensity: number;
  ringLight: RingLightConfig;
}): string {
  const spec = CAMERA_FILTERS.find((f) => f.id === params.filterId) ?? CAMERA_FILTERS[0];
  const ringMetrics = computeRingLightStyle(params.ringLight);

  // Exposure EV step adjustment (-4 to +4 maps to 0.68x .. 1.38x brightness)
  const evMultiplier = 1 + Math.max(-4, Math.min(4, params.exposureEv)) * 0.085;
  // Studio light slightly increases subject contrast
  const studioContrast = params.studioLight ? 1 + params.studioLightIntensity * 0.12 : 1;

  // Multiply by ringMetrics.brightnessMultiplier so turning Ring Light ON always increases brightness
  const brightness = (
    spec.brightness *
    evMultiplier *
    ringMetrics.brightnessMultiplier
  ).toFixed(3);
  const contrast = (spec.contrast * studioContrast).toFixed(3);
  const saturate = spec.saturate.toFixed(3);

  const parts = [
    `brightness(${brightness})`,
    `contrast(${contrast})`,
    `saturate(${saturate})`,
  ];

  if (spec.sepia > 0) parts.push(`sepia(${spec.sepia})`);
  if (spec.grayscale > 0) parts.push(`grayscale(${spec.grayscale})`);
  if (spec.hueRotateDeg !== 0) parts.push(`hue-rotate(${spec.hueRotateDeg}deg)`);

  return parts.join(' ');
}

/**
 * Normalizes a click inside a viewfinder bounding box to [0..1, 0..1] coordinates
 * for Android Camera2 MeteringRectangle AF/AE regions. Accounts for horizontal mirroring.
 */
export function normalizeTapCoordinates(
  clientX: number,
  clientY: number,
  rect: { left: number; top: number; width: number; height: number },
  mirrored: boolean
): { x: number; y: number; displayX: number; displayY: number } {
  if (rect.width <= 0 || rect.height <= 0) {
    return { x: 0.5, y: 0.5, displayX: 0.5, displayY: 0.5 };
  }
  const rawX = Math.max(0, Math.min(1, (clientX - rect.left) / rect.width));
  const rawY = Math.max(0, Math.min(1, (clientY - rect.top) / rect.height));
  const sensorX = mirrored ? 1 - rawX : rawX;

  return {
    x: Number(sensorX.toFixed(4)),
    y: Number(rawY.toFixed(4)),
    displayX: Number(rawX.toFixed(4)),
    displayY: Number(rawY.toFixed(4)),
  };
}

/**
 * Computes the optical zoom & Center Stage transform matrix parameters.
 */
export function computeViewportTransform(params: {
  zoom: number;
  mirrored: boolean;
  centerStage: boolean;
  subjectOffsetX: number; // -1..1
  subjectOffsetY: number; // -1..1
}): {
  scale: number;
  translateXPercent: number;
  translateYPercent: number;
  mirrorScaleX: 1 | -1;
} {
  const clampedZoom = Math.max(0.5, Math.min(10.0, params.zoom));
  const visualScale = clampedZoom < 1.0 ? 0.88 + (clampedZoom - 0.5) * 0.24 : clampedZoom;
  const centerStageScaleBoost = params.centerStage ? 1.08 : 1.0;
  const totalScale = Number((visualScale * centerStageScaleBoost).toFixed(3));

  const translateXPercent = params.centerStage
    ? Number((-params.subjectOffsetX * 5.5).toFixed(2))
    : 0;
  const translateYPercent = params.centerStage
    ? Number((-params.subjectOffsetY * 3.5).toFixed(2))
    : 0;

  return {
    scale: totalScale,
    translateXPercent,
    translateYPercent,
    mirrorScaleX: params.mirrored ? -1 : 1,
  };
}

// ── Wire Protocol v2.0 (14-byte header) Pure Verification Utilities ──

export const PACKET_HEADER_SIZE_V2 = 14;
export const FLAG_KEYFRAME = 0x01;
export const FLAG_CONFIG = 0x02;
export const FLAG_MIRRORED = 0x04;

export interface V2PacketHeader {
  packetType: number;
  flags: number;
  ptsUs: bigint;
  payloadLength: number;
}

export function encodeV2PacketHeader(header: V2PacketHeader): Uint8Array {
  const buf = new ArrayBuffer(PACKET_HEADER_SIZE_V2);
  const view = new DataView(buf);
  view.setUint8(0, header.packetType & 0xff);
  view.setUint8(1, header.flags & 0xff);
  view.setBigInt64(2, header.ptsUs, true);
  view.setUint32(10, header.payloadLength >>> 0, true);
  return new Uint8Array(buf);
}

export function decodeV2PacketHeader(bytes: Uint8Array): V2PacketHeader {
  if (bytes.byteLength < PACKET_HEADER_SIZE_V2) {
    throw new Error(`Invalid v2.0 packet header length: ${bytes.byteLength}`);
  }
  const view = new DataView(bytes.buffer, bytes.byteOffset, PACKET_HEADER_SIZE_V2);
  return {
    packetType: view.getUint8(0),
    flags: view.getUint8(1),
    ptsUs: view.getBigInt64(2, true),
    payloadLength: view.getUint32(10, true),
  };
}

export function computeAbrTargetBitrate(params: {
  currentBitrateBps: number;
  minBitrateBps: number;
  maxBitrateBps: number;
  congested: boolean;
  cleanSeconds: number;
}): number {
  const { currentBitrateBps, minBitrateBps, maxBitrateBps, congested, cleanSeconds } = params;
  if (congested) {
    const steppedDown = Math.round(currentBitrateBps * 0.8);
    return Math.max(minBitrateBps, Math.min(maxBitrateBps, steppedDown));
  }
  if (cleanSeconds >= 3 && currentBitrateBps < maxBitrateBps) {
    const rampedUp = Math.round(currentBitrateBps * 1.12);
    return Math.max(minBitrateBps, Math.min(maxBitrateBps, rampedUp));
  }
  return Math.max(minBitrateBps, Math.min(maxBitrateBps, currentBitrateBps));
}
