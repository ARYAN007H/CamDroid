import React, { useEffect, useRef, useState } from 'react';
import {
  CameraFilterId,
  CAMERA_FILTERS,
  RingLightConfig,
  StudioEffectsConfig,
  CameraHardwareState,
  buildCanvasFilterString,
  computeRingLightStyle,
  kelvinToRgb,
} from '../lib/cameraEngine';
import { Sparkles, Check, Sun } from 'lucide-react';

interface ContinuityViewfinderProps {
  activeFilter: CameraFilterId;
  onSelectFilter?: (filterId: CameraFilterId) => void;
  showFilterGrid?: boolean;
  ringLight: RingLightConfig;
  studioEffects: StudioEffectsConfig;
  hardware: CameraHardwareState;
  sourceMode: 'phone_continuity' | 'local_webcam';
  isRecording: boolean;
  recordingSeconds: number;
  countdownValue: number | null;
  shutterFlash: boolean;
  onCanvasReady?: (canvas: HTMLCanvasElement | null) => void;
  onTapFocus?: (normX: number, normY: number) => void;
}

export const ContinuityViewfinder: React.FC<ContinuityViewfinderProps> = ({
  activeFilter,
  onSelectFilter,
  showFilterGrid = false,
  ringLight,
  studioEffects,
  hardware,
  sourceMode,
  isRecording,
  recordingSeconds,
  countdownValue,
  shutterFlash,
  onCanvasReady,
  onTapFocus,
}) => {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const [focusReticle, setFocusReticle] = useState<{ x: number; y: number; visible: boolean }>({
    x: 50,
    y: 50,
    visible: false,
  });
  const [webcamError, setWebcamError] = useState<string | null>(null);

  useEffect(() => {
    if (onCanvasReady) {
      onCanvasReady(canvasRef.current);
    }
  }, [onCanvasReady]);

  // Handle local UVC webcam mode if selected
  useEffect(() => {
    let stream: MediaStream | null = null;
    if (sourceMode === 'local_webcam') {
      navigator.mediaDevices
        ?.getUserMedia({ video: { width: 1280, height: 720, frameRate: 30 }, audio: false })
        .then((s) => {
          stream = s;
          setWebcamError(null);
          if (videoRef.current) {
            videoRef.current.srcObject = s;
            videoRef.current.play().catch(() => {});
          }
        })
        .catch(() => {
          setWebcamError('Using Continuity Studio Feed (No local UVC camera found)');
        });
    } else {
      setWebcamError(null);
    }

    return () => {
      if (stream) {
        stream.getTracks().forEach((t) => t.stop());
      }
    };
  }, [sourceMode]);

  // Real-time 20fps Studio Feed Render Pipeline with Balanced Functional Ring Light Exposure Boost
  useEffect(() => {
    const startTime = performance.now();

    const drawStudioSubject = (
      ctx: CanvasRenderingContext2D,
      cx: number,
      cy: number,
      scale: number,
      t: number,
      ringOn: boolean,
      ringIntensityNorm: number,
      ringColor: { r: number; g: number; b: number }
    ) => {
      ctx.save();
      const breathY = Math.sin(t * 1.6) * 2.0;
      const headTilt = Math.sin(t * 0.85) * 0.012;
      ctx.translate(cx, cy + breathY);
      ctx.scale(scale, scale);

      // Ring light dynamic fill boost factor (0.0 when OFF, up to 1.0 when ON at 100%)
      // Carefully calibrated so skin tones and highlights brighten naturally without clipping to white
      const boost = ringOn ? 0.25 + ringIntensityNorm * 0.55 : 0;

      // 1. Shoulders & Studio Jacket / Knit Crewneck
      const coatGrad = ctx.createLinearGradient(0, 80, 0, 320);
      if (ringOn) {
        coatGrad.addColorStop(
          0,
          `rgb(${Math.round(38 + boost * 28)}, ${Math.round(45 + boost * 28)}, ${Math.round(58 + boost * 32)})`
        );
        coatGrad.addColorStop(1, '#141820');
      } else {
        coatGrad.addColorStop(0, '#1b2029');
        coatGrad.addColorStop(1, '#0b0e13');
      }
      ctx.fillStyle = coatGrad;
      ctx.beginPath();
      ctx.moveTo(-215, 320);
      ctx.quadraticCurveTo(-188, 128, -80, 106);
      ctx.lineTo(80, 106);
      ctx.quadraticCurveTo(188, 128, 215, 320);
      ctx.closePath();
      ctx.fill();

      // Inner crew-neck tee collar
      ctx.fillStyle = ringOn ? '#2c3342' : '#1b202b';
      ctx.beginPath();
      ctx.moveTo(-54, 108);
      ctx.quadraticCurveTo(0, 148, 54, 108);
      ctx.quadraticCurveTo(0, 128, -54, 108);
      ctx.fill();

      // Collar rim specular highlight from Ring Light
      ctx.strokeStyle = ringOn
        ? `rgba(${ringColor.r}, ${ringColor.g}, ${ringColor.b}, ${0.22 + boost * 0.35})`
        : 'rgba(255,255,255,0.07)';
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.moveTo(-66, 112);
      ctx.quadraticCurveTo(0, 142, 66, 112);
      ctx.stroke();

      // 2. Neck & Natural Chin Shadow
      const neckR = Math.round(156 + boost * 26);
      const neckG = Math.round(116 + boost * 22);
      const neckB = Math.round(96 + boost * 18);
      const neckGrad = ctx.createLinearGradient(0, 50, 0, 124);
      neckGrad.addColorStop(0, `rgb(${neckR - 18}, ${neckG - 16}, ${neckB - 14})`);
      neckGrad.addColorStop(1, `rgb(${neckR}, ${neckG}, ${neckB})`);
      ctx.fillStyle = neckGrad;
      ctx.beginPath();
      ctx.roundRect(-34, 52, 68, 72, 22);
      ctx.fill();

      // Head Group
      ctx.rotate(headTilt);

      // Back hair silhouette
      ctx.fillStyle = ringOn ? '#1d202a' : '#11131a';
      ctx.beginPath();
      ctx.ellipse(0, -34, 82, 92, 0, 0, Math.PI * 2);
      ctx.fill();

      // Ears
      const earR = Math.round(164 + boost * 24);
      const earG = Math.round(124 + boost * 22);
      const earB = Math.round(104 + boost * 20);
      ctx.fillStyle = `rgb(${earR}, ${earG}, ${earB})`;
      ctx.beginPath();
      ctx.ellipse(-74, -14, 11, 22, -0.1, 0, Math.PI * 2);
      ctx.ellipse(74, -14, 11, 22, 0.1, 0, Math.PI * 2);
      ctx.fill();

      // 3. Face Skin Tone with Balanced Key-Light Fill (No white clipping!)
      const kelvinWarmthR = ringOn ? Math.round((ringColor.r - 235) * 0.12) : 0;
      const kelvinWarmthB = ringOn ? Math.round((ringColor.b - 225) * 0.12) : 0;

      const faceCenterR = Math.min(228, Math.round(176 + boost * 34 + kelvinWarmthR));
      const faceCenterG = Math.min(196, Math.round(136 + boost * 30));
      const faceCenterB = Math.min(176, Math.round(114 + boost * 28 + kelvinWarmthB));

      const faceEdgeR = Math.min(198, Math.round(134 + boost * 32 + kelvinWarmthR));
      const faceEdgeG = Math.min(162, Math.round(98 + boost * 28));
      const faceEdgeB = Math.min(142, Math.round(80 + boost * 26 + kelvinWarmthB));

      const faceGrad = ctx.createRadialGradient(-12, -28, 10, 0, -12, 110);
      faceGrad.addColorStop(0, `rgb(${faceCenterR}, ${faceCenterG}, ${faceCenterB})`);
      faceGrad.addColorStop(
        0.68,
        `rgb(${Math.round((faceCenterR + faceEdgeR) / 2)}, ${Math.round(
          (faceCenterG + faceEdgeG) / 2
        )}, ${Math.round((faceCenterB + faceEdgeB) / 2)})`
      );
      faceGrad.addColorStop(1, `rgb(${faceEdgeR}, ${faceEdgeG}, ${faceEdgeB})`);

      ctx.fillStyle = faceGrad;
      ctx.beginPath();
      ctx.ellipse(0, -18, 73, 94, 0, 0, Math.PI * 2);
      ctx.fill();

      // Soft cheek & nose bridge highlight when Ring Light is ON
      if (ringOn) {
        const keyGlow = ctx.createRadialGradient(-6, -32, 4, 0, -22, 76);
        keyGlow.addColorStop(
          0,
          `rgba(${ringColor.r}, ${ringColor.g}, ${ringColor.b}, ${(0.12 + boost * 0.14).toFixed(3)})`
        );
        keyGlow.addColorStop(1, 'rgba(255,255,255,0)');
        ctx.fillStyle = keyGlow;
        ctx.beginPath();
        ctx.ellipse(0, -18, 73, 94, 0, 0, Math.PI * 2);
        ctx.fill();

        // Subtle edge-light rim around the jawline & temples
        ctx.strokeStyle = `rgba(${ringColor.r}, ${ringColor.g}, ${ringColor.b}, ${(0.28 + boost * 0.35).toFixed(2)})`;
        ctx.lineWidth = 2.2;
        ctx.beginPath();
        ctx.ellipse(0, -18, 73.5, 94.5, 0, 0, Math.PI * 2);
        ctx.stroke();
      }

      // 4. Styled Top Hair Volume
      const hairGrad = ctx.createLinearGradient(0, -126, 0, -35);
      hairGrad.addColorStop(0, ringOn ? '#2d3242' : '#1b1e28');
      hairGrad.addColorStop(1, '#101218');
      ctx.fillStyle = hairGrad;
      ctx.beginPath();
      ctx.moveTo(-74, -28);
      ctx.quadraticCurveTo(-80, -102, -18, -118);
      ctx.quadraticCurveTo(38, -126, 68, -88);
      ctx.quadraticCurveTo(82, -62, 74, -28);
      ctx.quadraticCurveTo(54, -76, 12, -82);
      ctx.quadraticCurveTo(-44, -84, -74, -28);
      ctx.closePath();
      ctx.fill();

      // 5. Eyebrows
      ctx.strokeStyle = '#1f1916';
      ctx.lineWidth = 4.2;
      ctx.lineCap = 'round';
      ctx.beginPath();
      ctx.moveTo(-43, -38);
      ctx.quadraticCurveTo(-27, -45, -13, -39);
      ctx.moveTo(13, -39);
      ctx.quadraticCurveTo(27, -45, 43, -38);
      ctx.stroke();

      // 6. Eyes (Sclera + Iris + Ring Light Circular Catchlights)
      ctx.fillStyle = ringOn ? '#f4f5f7' : '#d8dce3';
      ctx.beginPath();
      ctx.ellipse(-27, -21, 11.5, 6.8, 0, 0, Math.PI * 2);
      ctx.ellipse(27, -21, 11.5, 6.8, 0, 0, Math.PI * 2);
      ctx.fill();

      // Iris & Pupil
      ctx.fillStyle = '#2b211b';
      ctx.beginPath();
      ctx.arc(-27, -21, 5.8, 0, Math.PI * 2);
      ctx.arc(27, -21, 5.8, 0, Math.PI * 2);
      ctx.fill();

      ctx.fillStyle = '#0d0b0a';
      ctx.beginPath();
      ctx.arc(-27, -21, 2.8, 0, Math.PI * 2);
      ctx.arc(27, -21, 2.8, 0, Math.PI * 2);
      ctx.fill();

      // Signature Ring Light Circular Catchlight in Eyes when Ring Light is ON
      if (ringOn) {
        ctx.strokeStyle = `rgba(${ringColor.r}, ${ringColor.g}, ${ringColor.b}, 0.92)`;
        ctx.lineWidth = 1.6;
        ctx.beginPath();
        ctx.arc(-26, -22, 3.8, 0, Math.PI * 2);
        ctx.stroke();
        ctx.beginPath();
        ctx.arc(28, -22, 3.8, 0, Math.PI * 2);
        ctx.stroke();
      } else {
        ctx.fillStyle = 'rgba(255, 255, 255, 0.65)';
        ctx.beginPath();
        ctx.arc(-25, -23, 1.8, 0, Math.PI * 2);
        ctx.fill();
        ctx.beginPath();
        ctx.arc(29, -23, 1.8, 0, Math.PI * 2);
        ctx.fill();
      }

      // 7. Nose & Natural Smile
      ctx.strokeStyle = 'rgba(78, 46, 32, 0.42)';
      ctx.lineWidth = 2.4;
      ctx.beginPath();
      ctx.moveTo(0, -22);
      ctx.lineTo(-4, 7);
      ctx.quadraticCurveTo(0, 12, 6, 8);
      ctx.stroke();

      ctx.strokeStyle = ringOn ? '#9e4f44' : '#7d3e35';
      ctx.lineWidth = 3.5;
      ctx.beginPath();
      ctx.moveTo(-21, 31);
      ctx.quadraticCurveTo(0, 42, 21, 31);
      ctx.stroke();

      ctx.restore();
    };

    const renderFrame = (now: number) => {
      const canvas = canvasRef.current;
      if (!canvas) return;
      const ctx = canvas.getContext('2d');
      if (!ctx) return;

      const t = (now - startTime) / 1000;
      const w = canvas.width;
      const h = canvas.height;

      const ringOn = studioEffects.edgeLight.enabled;
      const rawIntensity = ringLight.intensity > 1 ? ringLight.intensity / 100 : ringLight.intensity;
      const ringIntensityNorm = Math.max(0, Math.min(1, rawIntensity));
      const ringRgb = kelvinToRgb(ringLight.kelvin);

      ctx.save();
      ctx.clearRect(0, 0, w, h);

      // Center Stage auto-framing pan + optical zoom
      const zoomScale =
        hardware.zoom === 0.5 ? 0.88 : hardware.zoom === 1 ? 1.0 : hardware.zoom === 2 ? 1.32 : 1.75;
      const panX = studioEffects.centerStage ? Math.sin(t * 0.55) * 20 : 0;
      const panY = studioEffects.centerStage ? Math.cos(t * 0.4) * 9 : 0;

      ctx.translate(w / 2 + panX, h / 2 + panY);
      ctx.scale(zoomScale, zoomScale);
      ctx.translate(-w / 2, -h / 2);

      const videoEl = videoRef.current;
      if (sourceMode === 'local_webcam' && videoEl && videoEl.readyState >= 2) {
        ctx.drawImage(videoEl, 0, 0, w, h);
      } else {
        // 1. Studio Environment Backdrop (Noticeably dimmer when Ring Light is OFF vs warmly illuminated when ON)
        const bgGrad = ctx.createRadialGradient(w * 0.5, h * 0.42, 40, w * 0.5, h * 0.5, w * 0.78);
        if (ringOn) {
          // Room ambient fill lifted by the Ring Light screen illumination
          const roomLift = Math.round(14 + ringIntensityNorm * 22);
          bgGrad.addColorStop(0, `rgb(${32 + roomLift}, ${38 + roomLift}, ${52 + roomLift})`);
          bgGrad.addColorStop(
            0.65,
            `rgb(${16 + Math.round(roomLift * 0.45)}, ${20 + Math.round(roomLift * 0.45)}, ${
              28 + Math.round(roomLift * 0.45)
            })`
          );
          bgGrad.addColorStop(1, '#0b0e14');
        } else if (studioEffects.studioLight) {
          const dim = studioEffects.studioLightIntensity;
          bgGrad.addColorStop(0, `rgba(30, 36, 48, ${1 - dim * 0.35})`);
          bgGrad.addColorStop(0.55, `rgba(13, 16, 22, ${1 - dim * 0.2})`);
          bgGrad.addColorStop(1, '#050608');
        } else {
          bgGrad.addColorStop(0, '#1e2330');
          bgGrad.addColorStop(0.6, '#11141d');
          bgGrad.addColorStop(1, '#080a0e');
        }
        ctx.fillStyle = bgGrad;
        ctx.fillRect(0, 0, w, h);

        // 2. Architectural Studio Bokeh Orbs
        const bokehAlpha = studioEffects.portraitBokeh
          ? 0.24 + ((16 - studioEffects.portraitApertureFStop) / 14.6) * 0.3
          : 0.12;

        const orbs = [
          { x: w * 0.2, y: h * 0.26, r: 95, color: '255, 185, 115' },
          { x: w * 0.8, y: h * 0.22, r: 120, color: '110, 175, 255' },
          { x: w * 0.74, y: h * 0.72, r: 85, color: '255, 150, 95' },
          { x: w * 0.24, y: h * 0.74, r: 75, color: '90, 210, 245' },
        ];
        orbs.forEach((orb, idx) => {
          const ox = orb.x + Math.sin(t * 0.4 + idx) * 10;
          const oy = orb.y + Math.cos(t * 0.35 + idx) * 8;
          const g = ctx.createRadialGradient(ox, oy, 2, ox, oy, orb.r);
          g.addColorStop(0, `rgba(${orb.color}, ${bokehAlpha})`);
          g.addColorStop(1, `rgba(${orb.color}, 0)`);
          ctx.fillStyle = g;
          ctx.beginPath();
          ctx.arc(ox, oy, orb.r, 0, Math.PI * 2);
          ctx.fill();
        });

        // 3. Studio Subject with Dynamic Ring Light Exposure & Catchlights
        drawStudioSubject(ctx, w * 0.5, h * 0.54, 1.12, t, ringOn, ringIntensityNorm, ringRgb);

        // 4. Functional Ring Light Inward Rim Illumination Wash
        if (ringOn) {
          ctx.save();
          ctx.globalCompositeOperation = 'screen';
          const edgeWash = ctx.createRadialGradient(
            w * 0.5,
            h * 0.5,
            w * 0.28,
            w * 0.5,
            h * 0.5,
            w * 0.62
          );
          edgeWash.addColorStop(0, 'rgba(0,0,0,0)');
          edgeWash.addColorStop(
            1,
            `rgba(${ringRgb.r}, ${ringRgb.g}, ${ringRgb.b}, ${(0.1 + ringIntensityNorm * 0.16).toFixed(3)})`
          );
          ctx.fillStyle = edgeWash;
          ctx.fillRect(0, 0, w, h);
          ctx.restore();
        }

        // 5. Studio Light Contour Vignette when Studio Light is enabled
        if (studioEffects.studioLight) {
          const vig = ctx.createRadialGradient(w * 0.5, h * 0.46, 150, w * 0.5, h * 0.5, w * 0.66);
          const strength = studioEffects.studioLightIntensity * (ringOn ? 0.32 : 0.52);
          vig.addColorStop(0, 'rgba(0, 0, 0, 0)');
          vig.addColorStop(1, `rgba(0, 0, 0, ${strength})`);
          ctx.fillStyle = vig;
          ctx.fillRect(0, 0, w, h);
        }
      }

      // Desk View Picture-in-Picture (macOS Continuity Ultra-Wide Desk Projection)
      if (studioEffects.deskView) {
        ctx.save();
        const dw = 310;
        const dh = 186;
        const dx = w - dw - 28;
        const dy = h - dh - 28;

        ctx.fillStyle = 'rgba(20, 22, 28, 0.94)';
        ctx.strokeStyle = 'rgba(255, 255, 255, 0.22)';
        ctx.lineWidth = 1.5;
        ctx.beginPath();
        ctx.roundRect(dx, dy, dw, dh, 16);
        ctx.fill();
        ctx.stroke();

        ctx.fillStyle = '#241f1c';
        ctx.beginPath();
        ctx.roundRect(dx + 8, dy + 32, dw - 16, dh - 40, 10);
        ctx.fill();

        ctx.fillStyle = '#d4d6dc';
        ctx.beginPath();
        ctx.roundRect(dx + 48, dy + 48, 130, 46, 5);
        ctx.fill();

        ctx.fillStyle = '#f5f2eb';
        ctx.beginPath();
        ctx.roundRect(dx + 194, dy + 48, 86, 112, 6);
        ctx.fill();

        ctx.fillStyle = 'rgba(255,255,255,0.88)';
        ctx.font = '600 11px -apple-system, BlinkMacSystemFont, "SF Pro Text", sans-serif';
        ctx.fillText('Desk View · 0.5× Ultra Wide', dx + 14, dy + 21);
        ctx.restore();
      }

      ctx.restore();
    };

    renderFrame(performance.now());
    const intervalId = window.setInterval(() => {
      renderFrame(performance.now());
    }, 50);

    return () => window.clearInterval(intervalId);
  }, [activeFilter, studioEffects, hardware, sourceMode, ringLight]);

  const handleCanvasClick = (e: React.MouseEvent<HTMLDivElement>) => {
    if (showFilterGrid) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const xPct = ((e.clientX - rect.left) / rect.width) * 100;
    const yPct = ((e.clientY - rect.top) / rect.height) * 100;
    setFocusReticle({ x: xPct, y: yPct, visible: true });
    if (onTapFocus) {
      onTapFocus(
        Number(((e.clientX - rect.left) / rect.width).toFixed(3)),
        Number(((e.clientY - rect.top) / rect.height).toFixed(3))
      );
    }
    setTimeout(() => {
      setFocusReticle((prev) => ({ ...prev, visible: false }));
    }, 1800);
  };

  const ringStyle = computeRingLightStyle({
    ...ringLight,
    enabled: studioEffects.edgeLight.enabled,
  });

  const canvasFilterCss = buildCanvasFilterString({
    filterId: activeFilter,
    exposureEv: hardware.ev,
    studioLight: studioEffects.studioLight,
    studioLightIntensity: studioEffects.studioLightIntensity,
    ringLight: {
      ...ringLight,
      enabled: studioEffects.edgeLight.enabled,
    },
  });

  // Top-left Continuity pills (Ring Light has its own dedicated top-right telemetry badge when active, avoiding any collision)
  const activeEffectsList = [
    studioEffects.portraitBokeh && `Portrait f/${studioEffects.portraitApertureFStop.toFixed(1)}`,
    studioEffects.studioLight && 'Studio Light',
    studioEffects.centerStage && 'Center Stage',
    studioEffects.deskView && 'Desk View',
  ].filter(Boolean) as string[];

  const formatDuration = (sec: number) => {
    const mins = Math.floor(sec / 60);
    const rem = sec % 60;
    return `${String(mins).padStart(2, '0')}:${String(rem).padStart(2, '0')}`;
  };

  const borderPx = studioEffects.edgeLight.enabled
    ? Math.max(14, Math.round(ringLight.widthPx * 0.45))
    : 0;

  return (
    <div
      onClick={handleCanvasClick}
      className="relative w-full h-full flex items-center justify-center overflow-hidden rounded-[22px] select-none cursor-crosshair"
      style={{
        backgroundColor: studioEffects.edgeLight.enabled ? '#171920' : '#090A0F',
        boxShadow: studioEffects.edgeLight.enabled
          ? `0 0 0 2px ${ringStyle.borderColor}, 0 24px 70px rgba(0,0,0,0.75)`
          : '0 0 0 1px rgba(255,255,255,0.10), 0 24px 60px rgba(0,0,0,0.75)',
      }}
    >
      {/* Hidden video element for optional local webcam fallback */}
      <video ref={videoRef} className="hidden" playsInline muted />

      {/* Main Live Camera Canvas */}
      <canvas
        ref={canvasRef}
        width={1280}
        height={720}
        className="w-full h-full object-cover"
        style={{ filter: canvasFilterCss }}
      />

      {/* Apple macOS Continuity Edge Light (Virtual Ring Light Illumination Bar) */}
      {studioEffects.edgeLight.enabled && (
        <div
          className="pointer-events-none absolute inset-0 rounded-[22px] z-20"
          style={{
            border: `${borderPx}px solid ${ringStyle.borderColor}`,
            boxShadow: `inset 0 0 42px ${ringStyle.glowColor}, 0 0 28px ${ringStyle.glowColor}`,
          }}
        />
      )}

      {/* Top-Left macOS Continuity Active Effects Pill Bar */}
      {!showFilterGrid && (
        <div
          className="pointer-events-none absolute z-20 flex flex-wrap items-center gap-1.5"
          style={{
            top: studioEffects.edgeLight.enabled ? `${borderPx + 12}px` : '16px',
            left: studioEffects.edgeLight.enabled ? `${borderPx + 14}px` : '16px',
          }}
        >
          {activeEffectsList.map((label) => (
            <span
              key={label}
              className="px-2.5 py-1 rounded-full bg-black/60 backdrop-blur-xl border border-white/15 text-[11px] font-medium text-white/90 tracking-tight shadow-sm whitespace-nowrap"
            >
              {label}
            </span>
          ))}
          {webcamError && (
            <span className="px-2.5 py-1 rounded-full bg-amber-500/20 backdrop-blur-xl border border-amber-400/30 text-[11px] text-amber-200 whitespace-nowrap">
              {webcamError}
            </span>
          )}
        </div>
      )}

      {/* Top-Right Ring Light Nits & Exposure Boost Telemetry Pill (Zero Overlap!) */}
      {!showFilterGrid && studioEffects.edgeLight.enabled && (
        <div
          className="pointer-events-none absolute z-20 flex items-center gap-2 px-3 py-1 rounded-full bg-black/75 backdrop-blur-xl border border-amber-300/35 text-[11px] font-medium text-white shadow-lg whitespace-nowrap"
          style={{
            top: `${borderPx + 12}px`,
            right: `${borderPx + 14}px`,
          }}
        >
          <Sun className="w-3.5 h-3.5 text-amber-300 fill-amber-300/30 shrink-0" />
          <span>Ring Light ({ringLight.kelvin}K)</span>
          <span className="text-white/35">·</span>
          <span className="font-mono text-amber-300 tabular-nums">
            {ringStyle.displayBrightnessPercent}% Nits
          </span>
          <span className="text-white/35">·</span>
          <span className="font-mono text-[#30D158] tabular-nums">
            +{ringStyle.sensorEvGain} EV Boost
          </span>
        </div>
      )}

      {/* Top-Right Recording Timecode Pill */}
      {isRecording && (
        <div className="pointer-events-none absolute top-4 right-4 z-20 flex items-center gap-2 px-3 py-1 rounded-full bg-[#FF453A]/90 backdrop-blur-md text-white text-xs font-mono font-semibold tracking-wider shadow-lg whitespace-nowrap">
          <span className="w-2 h-2 rounded-full bg-white" />
          <span>REC {formatDuration(recordingSeconds)}</span>
        </div>
      )}

      {/* macOS Photo Booth 3x3 Interactive Live Effects Grid Overlay */}
      {showFilterGrid && (
        <div
          onClick={(e) => e.stopPropagation()}
          className="absolute inset-0 z-30 bg-[#0D0E12]/95 backdrop-blur-2xl p-6 flex flex-col justify-between"
        >
          <div className="flex items-center justify-between px-1 mb-3">
            <div className="flex items-center gap-2">
              <Sparkles className="w-4 h-4 text-[#0A84FF]" />
              <span className="text-[13px] font-semibold tracking-tight text-white">
                Choose a Camera Filter
              </span>
              <span className="text-[11px] text-white/45">
                macOS Photo Booth & Continuity Color Profiles
              </span>
            </div>
            <span className="text-[11px] text-white/50">Click any tile to apply live</span>
          </div>

          <div className="grid grid-cols-3 grid-rows-3 gap-3.5 flex-1 min-h-0">
            {CAMERA_FILTERS.slice(0, 9).map((filter) => {
              const isSelected = activeFilter === filter.id;
              return (
                <button
                  key={filter.id}
                  onClick={() => onSelectFilter && onSelectFilter(filter.id)}
                  className={`group relative rounded-2xl overflow-hidden border flex flex-col justify-between p-3 text-left ${
                    isSelected
                      ? 'border-[#0A84FF] ring-2 ring-[#0A84FF]/50 shadow-lg'
                      : 'border-white/12 hover:border-white/30'
                  }`}
                  style={{
                    background:
                      'radial-gradient(circle at 50% 38%, #343d52 0%, #171b24 65%, #0c0e14 100%)',
                  }}
                >
                  {/* Simulated Live Portrait Preview with that tile's exact CSS filter */}
                  <div
                    className="absolute inset-0 flex items-center justify-center pointer-events-none"
                    style={{
                      filter:
                        filter.cssFilter === 'none'
                          ? studioEffects.edgeLight.enabled
                            ? 'brightness(1.18)'
                            : 'none'
                          : `${filter.cssFilter} ${
                              studioEffects.edgeLight.enabled ? 'brightness(1.18)' : ''
                            }`,
                    }}
                  >
                    <div className="relative w-24 h-24 flex flex-col items-center justify-end">
                      <div className="absolute -top-1 -left-4 w-7 h-7 rounded-full bg-amber-400/25 blur-sm" />
                      <div className="absolute top-0 -right-5 w-8 h-8 rounded-full bg-sky-400/25 blur-sm" />
                      <div className="w-10 h-12 rounded-full bg-gradient-to-b from-[#d8a98c] to-[#a6765b] shadow-inner border border-white/15" />
                      <div className="w-20 h-8 -mt-1 rounded-t-full bg-gradient-to-b from-[#333947] to-[#191d26]" />
                    </div>
                  </div>

                  {/* Top right selected checkmark */}
                  <div className="relative z-10 flex justify-end">
                    {isSelected && (
                      <span className="w-5 h-5 rounded-full bg-[#0A84FF] text-white flex items-center justify-center shadow">
                        <Check className="w-3 h-3 stroke-[2.5]" />
                      </span>
                    )}
                  </div>

                  {/* Bottom Filter Name Pill (macOS Photo Booth style) */}
                  <div className="relative z-10 flex items-center justify-between px-2.5 py-1 rounded-lg bg-black/65 backdrop-blur-md border border-white/10">
                    <span className="text-[11px] font-medium text-white tracking-tight">
                      {filter.name}
                    </span>
                    <span className="text-[10px] text-white/50 font-mono">{filter.badge}</span>
                  </div>
                </button>
              );
            })}
          </div>
        </div>
      )}

      {/* Bottom-Left Optical Lens Switcher Telemetry */}
      {!showFilterGrid && (
        <div
          className="pointer-events-none absolute z-20 flex items-center gap-2 px-3 py-1.5 rounded-full bg-black/60 backdrop-blur-xl border border-white/12 text-[11px] font-mono text-white/80 tabular-nums whitespace-nowrap"
          style={{
            bottom: studioEffects.edgeLight.enabled ? `${borderPx + 12}px` : '16px',
            left: studioEffects.edgeLight.enabled ? `${borderPx + 14}px` : '16px',
          }}
        >
          <span>{hardware.resolution.toUpperCase()}</span>
          <span className="text-white/30">·</span>
          <span>{hardware.fps} FPS</span>
          <span className="text-white/30">·</span>
          <span className="text-[#0A84FF] font-semibold">{hardware.zoom}×</span>
          <span className="text-white/30">·</span>
          <span>
            EV{' '}
            {(
              hardware.ev + (studioEffects.edgeLight.enabled ? ringStyle.sensorEvGain : 0)
            ).toFixed(1)}
          </span>
        </div>
      )}

      {/* Tap-to-Focus AF/AE Reticle (Apple Camera Yellow Square) */}
      {focusReticle.visible && (
        <div
          className="pointer-events-none absolute w-16 h-16 -ml-8 -mt-8 border-[1.5px] border-[#FFD60A] rounded-lg z-20"
          style={{ left: `${focusReticle.x}%`, top: `${focusReticle.y}%` }}
        >
          <div className="absolute -top-5 left-1/2 -translate-x-1/2 px-1.5 py-0.5 rounded bg-[#FFD60A] text-black text-[9px] font-mono font-bold tracking-wider">
            AF/AE
          </div>
        </div>
      )}

      {/* Countdown Timer Overlay */}
      {countdownValue !== null && (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center bg-black/35 backdrop-blur-sm z-30">
          <span className="text-8xl font-bold text-white font-mono drop-shadow-2xl">
            {countdownValue}
          </span>
        </div>
      )}

      {/* Shutter Flash Overlay */}
      {shutterFlash && (
        <div className="pointer-events-none absolute inset-0 bg-white/90 z-40" />
      )}
    </div>
  );
};
