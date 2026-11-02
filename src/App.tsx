import React, { useState, useEffect, useRef, useCallback } from 'react';
import {
  Camera,
  Video,
  Sparkles,
  Sliders,
  Sun,
  Smartphone,
  Usb,
  Wifi,
  Aperture,
  Layers,
  Focus,
  MonitorUp,
  Download,
  Trash2,
  CheckCircle2,
  Zap,
  RefreshCw,
  Grid,
  Laptop,
  PackageCheck,
  ShieldCheck,
  X,
} from 'lucide-react';
import { ContinuityViewfinder } from './components/ContinuityViewfinder';
import {
  CAMERA_FILTERS,
  CameraFilterId,
  RingLightConfig,
  StudioEffectsConfig,
  CameraHardwareState,
  BackendUpgradeStatus,
  BACKEND_UPGRADES,
  computeRingLightStyle,
  kelvinToLabel,
} from './lib/cameraEngine';
import {
  getStreamMetrics,
  sendCameraControl,
  setRingLightBrightness,
  toggleVirtualCameraOutput,
  StreamMetrics,
} from './lib/cameraBridge';

interface CapturedShot {
  id: string;
  dataUrl: string;
  timestamp: string;
  filterName: string;
  type: 'photo' | 'burst';
}

const SAMPLE_SHOTS: CapturedShot[] = [
  {
    id: 'sample-1',
    dataUrl: '',
    timestamp: 'Just now',
    filterName: 'Original',
    type: 'photo',
  },
];

export function App() {
  // Primary Camera & Continuity State
  const [sourceMode, setSourceMode] = useState<'phone_continuity' | 'local_webcam'>('phone_continuity');
  const [activeFilter, setActiveFilter] = useState<CameraFilterId>('original');
  const [showFilterGrid, setShowFilterGrid] = useState<boolean>(false);
  const [captureMode, setCaptureMode] = useState<'photo' | 'burst' | 'video'>('photo');
  const [timerSeconds, setTimerSeconds] = useState<0 | 3 | 10>(0);
  const [inspectorTab, setInspectorTab] = useState<'effects' | 'ringlight' | 'pro' | 'native'>('effects');
  const [inspectorOpen, setInspectorOpen] = useState<boolean>(true);

  // Virtual Ring Light (Edge Light) Configuration
  const [ringLight, setRingLight] = useState<RingLightConfig>({
    enabled: false,
    intensity: 0.85,
    kelvin: 4800,
    widthPx: 38,
  });

  // Studio Effects State (macOS Continuity Camera parity)
  const [studioEffects, setStudioEffects] = useState<StudioEffectsConfig>({
    portraitBokeh: true,
    portraitApertureFStop: 2.2,
    studioLight: true,
    studioLightIntensity: 0.58,
    centerStage: true,
    deskView: false,
    edgeLight: {
      enabled: false,
      intensity: 0.85,
      kelvin: 4800,
      widthPx: 38,
    },
  });

  // Phone Hardware Camera2 Controls
  const [hardware, setHardware] = useState<CameraHardwareState>({
    lens: 'back',
    zoom: 1,
    ev: 0.0,
    iso: 'AUTO',
    shutter: '1/60',
    wb: 5200,
    torch: false,
    afMode: 'continuous',
    resolution: '1080p',
    fps: 60,
  });

  // OBS / v4l2loopback / CoreMediaIO Virtual Camera Output State
  const [virtualCamActive, setVirtualCamActive] = useState<boolean>(true);

  // Capture / Recording / Filmstrip State
  const [isRecording, setIsRecording] = useState<boolean>(false);
  const [recordingSeconds, setRecordingSeconds] = useState<number>(0);
  const [countdownValue, setCountdownValue] = useState<number | null>(null);
  const [shutterFlash, setShutterFlash] = useState<boolean>(false);
  const [shots, setShots] = useState<CapturedShot[]>(SAMPLE_SHOTS);
  const [selectedShot, setSelectedShot] = useState<CapturedShot | null>(null);

  // Live Telemetry from Rust Desktop Companion / Wire Protocol v2.0
  const [metrics, setMetrics] = useState<StreamMetrics>({
    fps: 60.0,
    bitrateKbps: 8420,
    rttMs: 14.2,
    jitterMs: 1.4,
    droppedFrames: 0,
    batteryPercent: 88,
    temperatureC: 35.4,
    transport: 'USB-ADB',
    deviceModel: 'Google Pixel 8 Pro',
    virtualCamActive: true,
    resolution: '1920x1080',
    codec: 'H265',
  });

  const canvasRef = useRef<HTMLCanvasElement | null>(null);

  // Poll Stream Telemetry
  useEffect(() => {
    let mounted = true;
    const poll = async () => {
      const m = await getStreamMetrics();
      if (mounted) setMetrics(m);
    };
    poll();
    const id = window.setInterval(poll, 1500);
    return () => {
      mounted = false;
      window.clearInterval(id);
    };
  }, []);

  // Video Recording Timer
  useEffect(() => {
    if (!isRecording) {
      setRecordingSeconds(0);
      return;
    }
    const id = window.setInterval(() => {
      setRecordingSeconds((s) => s + 1);
    }, 1000);
    return () => window.clearInterval(id);
  }, [isRecording]);

  // Toggle Virtual Ring Light (Edge Light) & Trigger Physical OS Display Backlight + Camera Sensor EV Boost
  const handleToggleRingLight = useCallback(async () => {
    const nextEnabled = !studioEffects.edgeLight.enabled;
    const updatedRing: RingLightConfig = {
      ...ringLight,
      enabled: nextEnabled,
    };
    setRingLight(updatedRing);
    setStudioEffects((prev) => ({
      ...prev,
      edgeLight: updatedRing,
    }));
    if (nextEnabled) {
      setInspectorTab('ringlight');
    }
    await setRingLightBrightness(nextEnabled, updatedRing.intensity);
  }, [studioEffects.edgeLight.enabled, ringLight]);

  // Sync Ring Light Intensity Changes to Native Display Backlight & Phone Camera Sensor EV
  const handleRingLightIntensityChange = async (newIntensityPercent: number) => {
    const norm = Number((newIntensityPercent / 100).toFixed(2));
    const updated: RingLightConfig = {
      ...ringLight,
      intensity: norm,
      enabled: studioEffects.edgeLight.enabled,
    };
    setRingLight(updated);
    setStudioEffects((prev) => ({
      ...prev,
      edgeLight: updated,
    }));
    if (studioEffects.edgeLight.enabled) {
      await setRingLightBrightness(true, norm);
    }
  };

  // Capture Current Canvas Frame to Filmstrip
  const captureFrameNow = useCallback(
    (type: 'photo' | 'burst' = 'photo') => {
      setShutterFlash(true);
      setTimeout(() => setShutterFlash(false), 140);

      const canvas = canvasRef.current;
      const dataUrl = canvas ? canvas.toDataURL('image/png') : '';
      const filterObj = CAMERA_FILTERS.find((f) => f.id === activeFilter);
      const newShot: CapturedShot = {
        id: `shot-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
        dataUrl,
        timestamp: new Date().toLocaleTimeString([], {
          hour: '2-digit',
          minute: '2-digit',
          second: '2-digit',
        }),
        filterName: filterObj?.name || 'Original',
        type,
      };
      setShots((prev) => [newShot, ...prev.slice(0, 11)]);
    },
    [activeFilter]
  );

  // Trigger Shutter (Photo, 4-Photo Burst, or Video Record)
  const handleShutterTrigger = () => {
    if (showFilterGrid) {
      setShowFilterGrid(false);
    }

    if (captureMode === 'video') {
      setIsRecording((prev) => !prev);
      return;
    }

    const executeShot = () => {
      if (captureMode === 'photo') {
        captureFrameNow('photo');
      } else if (captureMode === 'burst') {
        captureFrameNow('burst');
        setTimeout(() => captureFrameNow('burst'), 220);
        setTimeout(() => captureFrameNow('burst'), 440);
        setTimeout(() => captureFrameNow('burst'), 660);
      }
    };

    if (timerSeconds === 0) {
      executeShot();
    } else {
      let rem = timerSeconds;
      setCountdownValue(rem);
      const timerId = window.setInterval(() => {
        rem -= 1;
        if (rem <= 0) {
          window.clearInterval(timerId);
          setCountdownValue(null);
          executeShot();
        } else {
          setCountdownValue(rem);
        }
      }, 1000);
    }
  };

  const handleLensZoom = async (zoom: 0.5 | 1 | 2 | 5) => {
    setHardware((prev) => ({ ...prev, zoom }));
    await sendCameraControl({ action: 'set_zoom', value: zoom });
  };

  const handleEvChange = async (ev: number) => {
    setHardware((prev) => ({ ...prev, ev }));
    await sendCameraControl({ action: 'set_exposure', value: Math.round(ev * 2) });
  };

  const handleToggleVirtualCam = async () => {
    const next = !virtualCamActive;
    setVirtualCamActive(next);
    await toggleVirtualCameraOutput(next);
  };

  const ringStyle = computeRingLightStyle({
    ...ringLight,
    enabled: studioEffects.edgeLight.enabled,
  });

  const activeFilterMeta = CAMERA_FILTERS.find((f) => f.id === activeFilter) || CAMERA_FILTERS[0];
  const intensityPercent = Math.round(
    (ringLight.intensity > 1 ? ringLight.intensity / 100 : ringLight.intensity) * 100
  );

  return (
    <div className="h-screen w-screen overflow-hidden bg-[#0B0C10] text-[#F5F5F7] flex flex-col select-none font-sans antialiased">
      {/* =====================================================================
          1. NATIVE macOS WINDOW TITLEBAR & CONTINUITY STATUS BAR (48px)
      ====================================================================== */}
      <header className="h-12 px-4 bg-[#16171D]/90 backdrop-blur-2xl border-b border-white/[0.08] flex items-center justify-between gap-3 shrink-0 z-30 whitespace-nowrap">
        {/* Left: macOS Traffic Lights + Continuity Camera Active Pill */}
        <div className="flex items-center gap-3 shrink-0">
          {/* Authentic macOS Window Controls */}
          <div className="flex items-center gap-2 shrink-0">
            <button
              title="Close Window"
              className="w-3 h-3 rounded-full bg-[#FF5F56] border border-black/20 hover:brightness-110 transition"
            />
            <button
              title="Minimize"
              className="w-3 h-3 rounded-full bg-[#FFBD2E] border border-black/20 hover:brightness-110 transition"
            />
            <button
              title="Toggle Fullscreen"
              onClick={() => {
                if (!document.fullscreenElement) {
                  document.documentElement.requestFullscreen?.().catch(() => {});
                } else {
                  document.exitFullscreen?.().catch(() => {});
                }
              }}
              className="w-3 h-3 rounded-full bg-[#27C93F] border border-black/20 hover:brightness-110 transition"
            />
          </div>

          <div className="h-4 w-[1px] bg-white/10 shrink-0" />

          {/* Apple Green Camera Indicator Dot + App Title */}
          <div className="flex items-center gap-2 shrink-0">
            <span className="w-2 h-2 rounded-full bg-[#30D158] shadow-[0_0_8px_#30D158]" />
            <span className="text-[13px] font-semibold tracking-tight text-white">
              CamDroid Studio
            </span>
          </div>

          {/* Device Source Switcher Pill */}
          <div className="flex items-center bg-[#242630] p-0.5 rounded-lg border border-white/[0.08] text-[11px] shrink-0">
            <button
              onClick={() => setSourceMode('phone_continuity')}
              className={`flex items-center gap-1.5 px-2.5 py-1 rounded-md font-medium transition whitespace-nowrap ${
                sourceMode === 'phone_continuity'
                  ? 'bg-[#343746] text-white shadow-sm'
                  : 'text-white/55 hover:text-white/85'
              }`}
            >
              <Smartphone className="w-3 h-3 text-[#30D158] shrink-0" />
              <span>Pixel 8 Pro</span>
            </button>
            <button
              onClick={() => setSourceMode('local_webcam')}
              className={`flex items-center gap-1.5 px-2.5 py-1 rounded-md font-medium transition whitespace-nowrap ${
                sourceMode === 'local_webcam'
                  ? 'bg-[#343746] text-white shadow-sm'
                  : 'text-white/55 hover:text-white/85'
              }`}
            >
              <Laptop className="w-3 h-3 text-[#0A84FF] shrink-0" />
              <span>Mac / PC</span>
            </button>
          </div>

          {/* Transport & Latency Telemetry Pill */}
          <div className="hidden 2xl:flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-white/[0.04] border border-white/[0.07] text-[11px] font-mono text-white/70 tabular-nums shrink-0">
            {metrics.transport === 'USB-ADB' ? (
              <Usb className="w-3 h-3 text-[#30D158] shrink-0" />
            ) : (
              <Wifi className="w-3 h-3 text-[#0A84FF] shrink-0" />
            )}
            <span>{metrics.transport}</span>
            <span className="text-white/25">·</span>
            <span className="text-[#30D158]">{metrics.rttMs.toFixed(1)}ms</span>
            <span className="text-white/25">·</span>
            <span>{Math.round(metrics.fps)}fps</span>
          </div>
        </div>

        {/* Center: Native macOS Segmented Mode Bar */}
        <div className="hidden md:flex items-center bg-[#22242E] p-0.5 rounded-lg border border-white/[0.08] text-[12px] shrink-0">
          <button
            onClick={() => setShowFilterGrid(false)}
            className={`px-3 py-1 rounded-[6px] font-medium transition whitespace-nowrap ${
              !showFilterGrid
                ? 'bg-[#3A3D4D] text-white shadow-sm'
                : 'text-white/60 hover:text-white'
            }`}
          >
            Camera View
          </button>
          <button
            onClick={() => setShowFilterGrid(true)}
            className={`flex items-center gap-1.5 px-3 py-1 rounded-[6px] font-medium transition whitespace-nowrap ${
              showFilterGrid
                ? 'bg-[#0A84FF] text-white shadow-sm'
                : 'text-white/60 hover:text-white'
            }`}
          >
            <Grid className="w-3.5 h-3.5 shrink-0" />
            <span>Effects (3×3)</span>
          </button>
          <button
            onClick={() => {
              setInspectorOpen(true);
              setInspectorTab('native');
            }}
            className={`flex items-center gap-1.5 px-3 py-1 rounded-[6px] font-medium transition whitespace-nowrap ${
              inspectorOpen && inspectorTab === 'native'
                ? 'bg-[#3A3D4D] text-white shadow-sm'
                : 'text-white/60 hover:text-white'
            }`}
          >
            <PackageCheck className="w-3.5 h-3.5 text-[#30D158] shrink-0" />
            <span>Native OS Builds</span>
          </button>
        </div>

        {/* Right: 1-Click Functional Ring Light Boost + OBS Virtual Camera + Video Effects Toggle */}
        <div className="flex items-center gap-1.5 shrink-0">
          {/* Functional Ring Light (Edge Light) Toggle with Live Nits & EV Boost Readout */}
          <button
            onClick={handleToggleRingLight}
            className={`flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-[12px] font-medium border transition-all whitespace-nowrap shrink-0 ${
              studioEffects.edgeLight.enabled
                ? 'bg-amber-400/20 border-amber-300/60 text-amber-200 shadow-[0_0_20px_rgba(251,191,36,0.28)]'
                : 'bg-white/[0.05] border-white/[0.1] text-white/75 hover:bg-white/[0.1]'
            }`}
          >
            <Sun
              className={`w-3.5 h-3.5 shrink-0 ${
                studioEffects.edgeLight.enabled
                  ? 'text-amber-300 fill-amber-300/40'
                  : 'text-white/60'
              }`}
            />
            <span>Ring Light</span>
            {studioEffects.edgeLight.enabled ? (
              <span className="px-1.5 py-0.5 rounded bg-amber-300 text-black font-mono text-[10px] font-bold tabular-nums whitespace-nowrap">
                {ringStyle.displayBrightnessPercent}% · +{ringStyle.sensorEvGain} EV
              </span>
            ) : (
              <span className="text-[10px] font-mono text-white/40">OFF</span>
            )}
          </button>

          {/* OBS Virtual Camera Bridge Toggle */}
          <button
            onClick={handleToggleVirtualCam}
            className={`hidden sm:flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-[11px] font-medium border transition whitespace-nowrap shrink-0 ${
              virtualCamActive
                ? 'bg-[#30D158]/15 border-[#30D158]/40 text-[#30D158]'
                : 'bg-white/[0.04] border-white/[0.08] text-white/55'
            }`}
            title="Broadcast camera feed to OBS Studio, Zoom, FaceTime, and Teams"
          >
            <MonitorUp className="w-3.5 h-3.5 shrink-0" />
            <span>OBS Cam</span>
          </button>

          {/* macOS Control Center Video Effects Sidebar Toggle */}
          <button
            onClick={() => setInspectorOpen((o) => !o)}
            className={`flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-[12px] font-medium border transition whitespace-nowrap shrink-0 ${
              inspectorOpen
                ? 'bg-[#0A84FF] border-[#0A84FF] text-white'
                : 'bg-white/[0.05] border-white/[0.1] text-white/75 hover:bg-white/[0.1]'
            }`}
          >
            <Sliders className="w-3.5 h-3.5 shrink-0" />
            <span>Video Effects</span>
          </button>
        </div>
      </header>

      {/* =====================================================================
          2. MAIN STUDIO WORKSPACE (VIEWFINDER STAGE + CONTROL CENTER SIDEBAR)
      ====================================================================== */}
      <div className="flex-1 min-h-0 flex overflow-hidden relative">
        {/* Center Viewfinder Stage */}
        <main className="flex-1 min-w-0 h-full p-4 flex flex-col items-center justify-center relative">
          <div className="w-full h-full max-w-[1360px] flex items-center justify-center">
            <ContinuityViewfinder
              activeFilter={activeFilter}
              onSelectFilter={(fid) => {
                setActiveFilter(fid);
                setShowFilterGrid(false);
              }}
              showFilterGrid={showFilterGrid}
              ringLight={ringLight}
              studioEffects={studioEffects}
              hardware={hardware}
              sourceMode={sourceMode}
              isRecording={isRecording}
              recordingSeconds={recordingSeconds}
              countdownValue={countdownValue}
              shutterFlash={shutterFlash}
              onCanvasReady={(c) => {
                canvasRef.current = c;
              }}
              onTapFocus={(nx, ny) => {
                sendCameraControl({ action: 'tap_focus', x: nx, y: ny });
              }}
            />
          </div>

          {/* Floating Optical Lens Selector Pill (iOS / Continuity Camera 0.5x / 1x / 2x / 5x) */}
          {!showFilterGrid && (
            <div className="absolute bottom-7 left-1/2 -translate-x-1/2 z-20 flex items-center gap-1 p-1 rounded-full bg-black/65 backdrop-blur-2xl border border-white/15 shadow-xl">
              {([0.5, 1, 2, 5] as const).map((z) => (
                <button
                  key={z}
                  onClick={() => handleLensZoom(z)}
                  className={`w-8 h-8 rounded-full text-[11px] font-mono font-bold transition-all flex items-center justify-center tabular-nums ${
                    hardware.zoom === z
                      ? 'bg-[#FFD60A] text-black scale-105 shadow'
                      : 'text-white/80 hover:bg-white/10'
                  }`}
                >
                  {z}×
                </button>
              ))}
            </div>
          )}
        </main>

        {/* Right macOS Control Center "Video Effects" Translucent Inspector */}
        {inspectorOpen && (
          <aside className="w-[350px] shrink-0 h-full bg-[#181920]/90 backdrop-blur-2xl border-l border-white/[0.08] flex flex-col z-20">
            {/* Inspector Segmented Navigation */}
            <div className="p-3 border-b border-white/[0.07]">
              <div className="grid grid-cols-4 gap-1 bg-[#101116] p-1 rounded-xl border border-white/[0.06] text-[11px]">
                <button
                  onClick={() => setInspectorTab('effects')}
                  className={`py-1.5 rounded-lg font-medium transition ${
                    inspectorTab === 'effects'
                      ? 'bg-[#2C2F3D] text-white shadow'
                      : 'text-white/55 hover:text-white'
                  }`}
                >
                  Effects
                </button>
                <button
                  onClick={() => setInspectorTab('ringlight')}
                  className={`py-1.5 rounded-lg font-medium transition flex items-center justify-center gap-1 ${
                    inspectorTab === 'ringlight'
                      ? 'bg-[#2C2F3D] text-amber-300 shadow'
                      : 'text-white/55 hover:text-white'
                  }`}
                >
                  <span>Ring Light</span>
                  {studioEffects.edgeLight.enabled && (
                    <span className="w-1.5 h-1.5 rounded-full bg-amber-400" />
                  )}
                </button>
                <button
                  onClick={() => setInspectorTab('pro')}
                  className={`py-1.5 rounded-lg font-medium transition ${
                    inspectorTab === 'pro'
                      ? 'bg-[#2C2F3D] text-white shadow'
                      : 'text-white/55 hover:text-white'
                  }`}
                >
                  Optics
                </button>
                <button
                  onClick={() => setInspectorTab('native')}
                  className={`py-1.5 rounded-lg font-medium transition ${
                    inspectorTab === 'native'
                      ? 'bg-[#2C2F3D] text-[#30D158] shadow'
                      : 'text-white/55 hover:text-white'
                  }`}
                >
                  OS Build
                </button>
              </div>
            </div>

            {/* Inspector Scrollable Body */}
            <div className="flex-1 overflow-y-auto p-3.5 space-y-3.5">
              {/* TAB 1: CONTINUITY VIDEO EFFECTS & COLOR FILTERS */}
              {inspectorTab === 'effects' && (
                <>
                  {/* Continuity Video Effects Tiles (macOS Control Center style) */}
                  <div className="bg-[#22242E]/90 rounded-2xl p-3.5 border border-white/[0.08] space-y-3">
                    <div className="flex items-center justify-between">
                      <span className="text-[12px] font-semibold text-white">
                        Continuity Video Effects
                      </span>
                      <span className="text-[10px] font-mono text-[#30D158]">Neural ISP</span>
                    </div>

                    <div className="grid grid-cols-2 gap-2">
                      {/* Portrait Mode */}
                      <button
                        onClick={() =>
                          setStudioEffects((s) => ({
                            ...s,
                            portraitBokeh: !s.portraitBokeh,
                          }))
                        }
                        className={`p-2.5 rounded-xl border text-left transition flex flex-col justify-between ${
                          studioEffects.portraitBokeh
                            ? 'bg-[#0A84FF]/20 border-[#0A84FF] text-white'
                            : 'bg-white/[0.03] border-white/[0.07] text-white/65 hover:bg-white/[0.06]'
                        }`}
                      >
                        <Aperture
                          className={`w-4 h-4 mb-1.5 ${
                            studioEffects.portraitBokeh ? 'text-[#0A84FF]' : 'text-white/45'
                          }`}
                        />
                        <div className="text-[12px] font-semibold">Portrait</div>
                        <div className="text-[10px] text-white/50">
                          {studioEffects.portraitBokeh ? 'Active · Depth Bokeh' : 'Off'}
                        </div>
                      </button>

                      {/* Studio Light */}
                      <button
                        onClick={() =>
                          setStudioEffects((s) => ({
                            ...s,
                            studioLight: !s.studioLight,
                          }))
                        }
                        className={`p-2.5 rounded-xl border text-left transition flex flex-col justify-between ${
                          studioEffects.studioLight
                            ? 'bg-[#0A84FF]/20 border-[#0A84FF] text-white'
                            : 'bg-white/[0.03] border-white/[0.07] text-white/65 hover:bg-white/[0.06]'
                        }`}
                      >
                        <Sparkles
                          className={`w-4 h-4 mb-1.5 ${
                            studioEffects.studioLight ? 'text-[#0A84FF]' : 'text-white/45'
                          }`}
                        />
                        <div className="text-[12px] font-semibold">Studio Light</div>
                        <div className="text-[10px] text-white/50">
                          {studioEffects.studioLight ? 'Active · Face Lit' : 'Off'}
                        </div>
                      </button>

                      {/* Center Stage */}
                      <button
                        onClick={() =>
                          setStudioEffects((s) => ({
                            ...s,
                            centerStage: !s.centerStage,
                          }))
                        }
                        className={`p-2.5 rounded-xl border text-left transition flex flex-col justify-between ${
                          studioEffects.centerStage
                            ? 'bg-[#30D158]/20 border-[#30D158] text-white'
                            : 'bg-white/[0.03] border-white/[0.07] text-white/65 hover:bg-white/[0.06]'
                        }`}
                      >
                        <Focus
                          className={`w-4 h-4 mb-1.5 ${
                            studioEffects.centerStage ? 'text-[#30D158]' : 'text-white/45'
                          }`}
                        />
                        <div className="text-[12px] font-semibold">Center Stage</div>
                        <div className="text-[10px] text-white/50">
                          {studioEffects.centerStage ? 'Auto-Framing' : 'Off'}
                        </div>
                      </button>

                      {/* Desk View */}
                      <button
                        onClick={() =>
                          setStudioEffects((s) => ({
                            ...s,
                            deskView: !s.deskView,
                          }))
                        }
                        className={`p-2.5 rounded-xl border text-left transition flex flex-col justify-between ${
                          studioEffects.deskView
                            ? 'bg-amber-400/20 border-amber-400 text-white'
                            : 'bg-white/[0.03] border-white/[0.07] text-white/65 hover:bg-white/[0.06]'
                        }`}
                      >
                        <Layers
                          className={`w-4 h-4 mb-1.5 ${
                            studioEffects.deskView ? 'text-amber-300' : 'text-white/45'
                          }`}
                        />
                        <div className="text-[12px] font-semibold">Desk View</div>
                        <div className="text-[10px] text-white/50">
                          {studioEffects.deskView ? '0.5× Ultra Wide PiP' : 'Off'}
                        </div>
                      </button>
                    </div>

                    {/* Sliders for Portrait Blur & Studio Light Contour */}
                    <div className="pt-1 space-y-2.5 border-t border-white/[0.06]">
                      <div>
                        <div className="flex justify-between text-[11px] mb-1">
                          <span className="text-white/70">Portrait Depth (Aperture)</span>
                          <span className="font-mono text-white/90 tabular-nums">
                            f/{studioEffects.portraitApertureFStop.toFixed(1)}
                          </span>
                        </div>
                        <input
                          type="range"
                          min={1.4}
                          max={5.6}
                          step={0.2}
                          value={studioEffects.portraitApertureFStop}
                          onChange={(e) =>
                            setStudioEffects((s) => ({
                              ...s,
                              portraitApertureFStop: Number(e.target.value),
                            }))
                          }
                          className="w-full accent-[#0A84FF] h-1.5 bg-white/10 rounded-lg cursor-pointer"
                        />
                      </div>

                      <div>
                        <div className="flex justify-between text-[11px] mb-1">
                          <span className="text-white/70">Studio Light Contour</span>
                          <span className="font-mono text-white/90 tabular-nums">
                            {Math.round(studioEffects.studioLightIntensity * 100)}%
                          </span>
                        </div>
                        <input
                          type="range"
                          min={20}
                          max={100}
                          value={Math.round(studioEffects.studioLightIntensity * 100)}
                          onChange={(e) =>
                            setStudioEffects((s) => ({
                              ...s,
                              studioLightIntensity: Number(e.target.value) / 100,
                            }))
                          }
                          className="w-full accent-[#0A84FF] h-1.5 bg-white/10 rounded-lg cursor-pointer"
                        />
                      </div>
                    </div>
                  </div>

                  {/* Apple Photo Booth Color Filters Swatch Grid */}
                  <div className="bg-[#22242E]/90 rounded-2xl p-3.5 border border-white/[0.08] space-y-2.5">
                    <div className="flex items-center justify-between">
                      <span className="text-[12px] font-semibold text-white">
                        Color Grade ({activeFilterMeta.name})
                      </span>
                      <button
                        onClick={() => setShowFilterGrid((g) => !g)}
                        className="text-[11px] text-[#0A84FF] hover:underline font-medium"
                      >
                        {showFilterGrid ? 'Close 3×3 Grid' : 'Open 3×3 Grid'}
                      </button>
                    </div>

                    <div className="grid grid-cols-3 gap-1.5">
                      {CAMERA_FILTERS.map((f) => {
                        const active = activeFilter === f.id;
                        return (
                          <button
                            key={f.id}
                            onClick={() => setActiveFilter(f.id)}
                            className={`px-2 py-2 rounded-xl border text-left transition ${
                              active
                                ? 'bg-[#0A84FF]/20 border-[#0A84FF] text-white'
                                : 'bg-white/[0.03] border-white/[0.06] text-white/70 hover:bg-white/[0.07]'
                            }`}
                          >
                            <div className="text-[11px] font-semibold truncate">{f.name}</div>
                            <div className="text-[9px] font-mono text-white/45">{f.badge}</div>
                          </button>
                        );
                      })}
                    </div>
                  </div>
                </>
              )}

              {/* TAB 2: FUNCTIONAL VIRTUAL RING LIGHT (EDGE LIGHT + HARDWARE NITS BOOST) */}
              {inspectorTab === 'ringlight' && (
                <div className="bg-[#22242E]/90 rounded-2xl p-3.5 border border-white/[0.08] space-y-3.5">
                  <div className="flex items-center justify-between">
                    <div>
                      <div className="text-[13px] font-semibold text-white">
                        Virtual Ring Light (Edge Light)
                      </div>
                      <div className="text-[11px] text-white/55">
                        Boosts physical screen nits + camera exposure
                      </div>
                    </div>
                    <button
                      onClick={handleToggleRingLight}
                      className={`px-3 py-1.5 rounded-full text-[11px] font-semibold transition ${
                        studioEffects.edgeLight.enabled
                          ? 'bg-amber-400 text-black shadow'
                          : 'bg-white/10 text-white/70 hover:bg-white/20'
                      }`}
                    >
                      {studioEffects.edgeLight.enabled ? 'ACTIVE' : 'TURN ON'}
                    </button>
                  </div>

                  {/* Live Brightness & Sensor Exposure Telemetry Readout */}
                  <div className="grid grid-cols-3 gap-2 p-2.5 rounded-xl bg-black/40 border border-white/[0.07] text-center">
                    <div>
                      <div className="text-[10px] text-white/45">Display Nits</div>
                      <div className="text-[13px] font-mono font-bold text-amber-300 tabular-nums">
                        {ringStyle.displayBrightnessPercent}%
                      </div>
                    </div>
                    <div>
                      <div className="text-[10px] text-white/45">Scene Gain</div>
                      <div className="text-[13px] font-mono font-bold text-[#30D158] tabular-nums">
                        {ringStyle.brightnessMultiplier}×
                      </div>
                    </div>
                    <div>
                      <div className="text-[10px] text-white/45">Sensor EV</div>
                      <div className="text-[13px] font-mono font-bold text-[#0A84FF] tabular-nums">
                        +{ringStyle.sensorEvGain} EV
                      </div>
                    </div>
                  </div>

                  {/* Quick Color Temperature Presets */}
                  <div className="grid grid-cols-3 gap-1.5">
                    {[
                      { label: 'Warm 3200K', k: 3200 },
                      { label: 'Studio 4800K', k: 4800 },
                      { label: 'Daylight 6000K', k: 6000 },
                    ].map((preset) => (
                      <button
                        key={preset.k}
                        onClick={() => {
                          const updated: RingLightConfig = {
                            ...ringLight,
                            kelvin: preset.k,
                            enabled: true,
                          };
                          setRingLight(updated);
                          setStudioEffects((s) => ({ ...s, edgeLight: updated }));
                          setRingLightBrightness(true, updated.intensity);
                        }}
                        className={`py-1.5 px-2 rounded-lg border text-[11px] font-medium transition ${
                          ringLight.kelvin === preset.k
                            ? 'bg-amber-400/20 border-amber-300 text-amber-200'
                            : 'bg-white/[0.04] border-white/[0.08] text-white/70 hover:bg-white/[0.08]'
                        }`}
                      >
                        {preset.label}
                      </button>
                    ))}
                  </div>

                  {/* Luminance Output Slider */}
                  <div>
                    <div className="flex justify-between text-[11px] mb-1">
                      <span className="text-white/75">Luminance & Exposure Boost</span>
                      <span className="font-mono text-amber-300 tabular-nums">
                        {intensityPercent}%
                      </span>
                    </div>
                    <input
                      type="range"
                      min={15}
                      max={100}
                      value={intensityPercent}
                      onChange={(e) => handleRingLightIntensityChange(Number(e.target.value))}
                      className="w-full accent-amber-400 h-1.5 bg-white/10 rounded-lg cursor-pointer"
                    />
                  </div>

                  {/* Color Temperature (Kelvin) Slider */}
                  <div>
                    <div className="flex justify-between text-[11px] mb-1">
                      <span className="text-white/75">
                        Color Temp ({kelvinToLabel(ringLight.kelvin)})
                      </span>
                      <span className="font-mono text-white/90 tabular-nums">
                        {ringLight.kelvin}K
                      </span>
                    </div>
                    <input
                      type="range"
                      min={2700}
                      max={6500}
                      step={100}
                      value={ringLight.kelvin}
                      onChange={(e) => {
                        const k = Number(e.target.value);
                        setRingLight((r) => ({ ...r, kelvin: k }));
                        setStudioEffects((s) => ({
                          ...s,
                          edgeLight: { ...s.edgeLight, kelvin: k },
                        }));
                      }}
                      className="w-full accent-amber-300 h-1.5 bg-gradient-to-r from-amber-500 via-amber-100 to-sky-300 rounded-lg cursor-pointer"
                    />
                  </div>

                  {/* Diffuser Border Width Slider */}
                  <div>
                    <div className="flex justify-between text-[11px] mb-1">
                      <span className="text-white/75">Edge Diffuser Width</span>
                      <span className="font-mono text-white/90 tabular-nums">
                        {ringLight.widthPx} px
                      </span>
                    </div>
                    <input
                      type="range"
                      min={16}
                      max={72}
                      value={ringLight.widthPx}
                      onChange={(e) => {
                        const w = Number(e.target.value);
                        setRingLight((r) => ({ ...r, widthPx: w }));
                        setStudioEffects((s) => ({
                          ...s,
                          edgeLight: { ...s.edgeLight, widthPx: w },
                        }));
                      }}
                      className="w-full accent-[#0A84FF] h-1.5 bg-white/10 rounded-lg cursor-pointer"
                    />
                  </div>
                </div>
              )}

              {/* TAB 3: CAMERA2 PRO OPTICS & HARDWARE ISP */}
              {inspectorTab === 'pro' && (
                <div className="bg-[#22242E]/90 rounded-2xl p-3.5 border border-white/[0.08] space-y-3.5">
                  <div className="flex items-center justify-between">
                    <span className="text-[12px] font-semibold text-white">
                      Android Camera2 Remote ISP
                    </span>
                    <button
                      onClick={() => {
                        const nextLens = hardware.lens === 'back' ? 'front' : 'back';
                        setHardware((h) => ({ ...h, lens: nextLens }));
                        sendCameraControl({ action: 'switch_camera' });
                      }}
                      className="flex items-center gap-1 px-2.5 py-1 rounded-lg bg-white/10 hover:bg-white/15 text-[11px] text-white"
                    >
                      <RefreshCw className="w-3 h-3" />
                      <span>{hardware.lens === 'back' ? 'Rear Main' : 'Front Lens'}</span>
                    </button>
                  </div>

                  {/* Exposure Compensation (EV) */}
                  <div>
                    <div className="flex justify-between text-[11px] mb-1">
                      <span className="text-white/75">Exposure Bias (EV)</span>
                      <span className="font-mono text-[#FFD60A] tabular-nums">
                        {hardware.ev >= 0 ? `+${hardware.ev.toFixed(1)}` : hardware.ev.toFixed(1)} EV
                      </span>
                    </div>
                    <input
                      type="range"
                      min={-2.0}
                      max={2.0}
                      step={0.2}
                      value={hardware.ev}
                      onChange={(e) => handleEvChange(Number(e.target.value))}
                      className="w-full accent-[#FFD60A] h-1.5 bg-white/10 rounded-lg cursor-pointer"
                    />
                  </div>

                  {/* Stream Resolution & FPS */}
                  <div className="grid grid-cols-2 gap-2">
                    <div className="p-2.5 rounded-xl bg-black/35 border border-white/[0.06]">
                      <div className="text-[10px] text-white/45 mb-1">Resolution</div>
                      <div className="flex gap-1">
                        {(['720p', '1080p', '4K'] as const).map((res) => (
                          <button
                            key={res}
                            onClick={() => setHardware((h) => ({ ...h, resolution: res }))}
                            className={`flex-1 py-1 rounded text-[10px] font-mono font-semibold ${
                              hardware.resolution === res
                                ? 'bg-[#0A84FF] text-white'
                                : 'bg-white/5 text-white/60'
                            }`}
                          >
                            {res}
                          </button>
                        ))}
                      </div>
                    </div>

                    <div className="p-2.5 rounded-xl bg-black/35 border border-white/[0.06]">
                      <div className="text-[10px] text-white/45 mb-1">Frame Rate</div>
                      <div className="flex gap-1">
                        {([30, 60] as const).map((fps) => (
                          <button
                            key={fps}
                            onClick={() => setHardware((h) => ({ ...h, fps }))}
                            className={`flex-1 py-1 rounded text-[10px] font-mono font-semibold ${
                              hardware.fps === fps
                                ? 'bg-[#0A84FF] text-white'
                                : 'bg-white/5 text-white/60'
                            }`}
                          >
                            {fps} fps
                          </button>
                        ))}
                      </div>
                    </div>
                  </div>

                  {/* Hardware Torch Toggle */}
                  <button
                    onClick={() => {
                      const next = !hardware.torch;
                      setHardware((h) => ({ ...h, torch: next }));
                      sendCameraControl({ action: 'toggle_flash', enabled: next });
                    }}
                    className={`w-full py-2 px-3 rounded-xl border text-[12px] font-medium flex items-center justify-between transition ${
                      hardware.torch
                        ? 'bg-amber-400/20 border-amber-400 text-amber-200'
                        : 'bg-white/[0.04] border-white/[0.08] text-white/75'
                    }`}
                  >
                    <span className="flex items-center gap-2">
                      <Zap className="w-3.5 h-3.5" />
                      <span>Phone Rear LED Torch</span>
                    </span>
                    <span className="font-mono text-[11px]">{hardware.torch ? 'ON' : 'OFF'}</span>
                  </button>
                </div>
              )}

              {/* TAB 4: NATIVE MULTI-OS RELEASES (macOS / Windows / Linux) & v2.0 ARCHITECTURE */}
              {inspectorTab === 'native' && (
                <div className="space-y-3">
                  {/* Native Multi-OS Release Matrix Card */}
                  <div className="bg-[#22242E]/90 rounded-2xl p-3.5 border border-white/[0.08] space-y-2.5">
                    <div className="flex items-center justify-between">
                      <span className="text-[12px] font-semibold text-white flex items-center gap-1.5">
                        <PackageCheck className="w-4 h-4 text-[#30D158]" />
                        <span>Native OS Release Workflow</span>
                      </span>
                      <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-[#30D158]/15 text-[#30D158]">
                        release.yml
                      </span>
                    </div>
                    <p className="text-[11px] text-white/60 leading-relaxed">
                      Built natively with Rust + Tauri v1 + FFmpeg & OS Display Backlight APIs via{' '}
                      <code className="text-white/90 font-mono">.github/workflows/release.yml</code>:
                    </p>
                    <div className="space-y-1.5 text-[11px]">
                      <div className="p-2 rounded-lg bg-black/35 border border-white/[0.06] flex items-center justify-between">
                        <span className="font-medium text-white">macOS (Apple Silicon & Intel)</span>
                        <span className="font-mono text-[10px] text-[#0A84FF]">.dmg · .app</span>
                      </div>
                      <div className="p-2 rounded-lg bg-black/35 border border-white/[0.06] flex items-center justify-between">
                        <span className="font-medium text-white">Windows 11 / 10 (x64)</span>
                        <span className="font-mono text-[10px] text-[#0A84FF]">.msi · .exe</span>
                      </div>
                      <div className="p-2 rounded-lg bg-black/35 border border-white/[0.06] flex items-center justify-between">
                        <span className="font-medium text-white">Linux (Ubuntu / Fedora / Arch)</span>
                        <span className="font-mono text-[10px] text-[#0A84FF]">.AppImage · .deb</span>
                      </div>
                      <div className="p-2 rounded-lg bg-black/35 border border-white/[0.06] flex items-center justify-between">
                        <span className="font-medium text-white">Android Companion App</span>
                        <span className="font-mono text-[10px] text-[#30D158]">CamDroid.apk</span>
                      </div>
                    </div>
                  </div>

                  {/* Verified 10 Backend Architecture Upgrades */}
                  <div className="bg-[#22242E]/90 rounded-2xl p-3.5 border border-white/[0.08] space-y-2">
                    <div className="flex items-center justify-between">
                      <span className="text-[12px] font-semibold text-white flex items-center gap-1.5">
                        <ShieldCheck className="w-4 h-4 text-[#0A84FF]" />
                        <span>Wire Protocol v2.0 Upgrades</span>
                      </span>
                      <span className="text-[10px] font-mono text-[#30D158]">10/10 Active</span>
                    </div>
                    <div className="space-y-1.5 max-h-[210px] overflow-y-auto pr-1">
                      {BACKEND_UPGRADES.map((up: BackendUpgradeStatus) => (
                        <div
                          key={up.id}
                          className="p-2 rounded-lg bg-black/30 border border-white/[0.05] flex items-start justify-between gap-2"
                        >
                          <div>
                            <div className="text-[11px] font-medium text-white flex items-center gap-1">
                              <CheckCircle2 className="w-3 h-3 text-[#30D158] shrink-0" />
                              <span>
                                #{up.id} {up.title}
                              </span>
                            </div>
                            <div className="text-[10px] text-white/45 font-mono mt-0.5">
                              {up.subsystem} · {up.metric}
                            </div>
                          </div>
                        </div>
                      ))}
                    </div>
                  </div>
                </div>
              )}
            </div>
          </aside>
        )}
      </div>

      {/* =====================================================================
          3. AUTHENTIC macOS PHOTO BOOTH BOTTOM SHUTTER & FILMSTRIP DOCK (84px)
      ====================================================================== */}
      <footer className="h-[84px] px-6 bg-[#16171D]/95 backdrop-blur-2xl border-t border-white/[0.08] flex items-center justify-between shrink-0 z-30">
        {/* Left: Capture Mode Segmented Control (Photo / 4-Burst / Video) + Timer */}
        <div className="flex items-center gap-3">
          <div className="flex items-center bg-[#232530] p-1 rounded-xl border border-white/[0.08] text-[12px]">
            <button
              onClick={() => setCaptureMode('photo')}
              className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg font-medium transition ${
                captureMode === 'photo'
                  ? 'bg-[#383B4C] text-white shadow-sm'
                  : 'text-white/55 hover:text-white'
              }`}
            >
              <Camera className="w-3.5 h-3.5" />
              <span>Photo</span>
            </button>
            <button
              onClick={() => setCaptureMode('burst')}
              className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg font-medium transition ${
                captureMode === 'burst'
                  ? 'bg-[#383B4C] text-white shadow-sm'
                  : 'text-white/55 hover:text-white'
              }`}
            >
              <Grid className="w-3.5 h-3.5" />
              <span>4-Burst</span>
            </button>
            <button
              onClick={() => setCaptureMode('video')}
              className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg font-medium transition ${
                captureMode === 'video'
                  ? 'bg-[#FF453A] text-white shadow-sm'
                  : 'text-white/55 hover:text-white'
              }`}
            >
              <Video className="w-3.5 h-3.5" />
              <span>Video</span>
            </button>
          </div>

          {/* Countdown Timer Pill */}
          <div className="hidden sm:flex items-center bg-[#232530] p-1 rounded-xl border border-white/[0.08] text-[11px] font-mono">
            {([0, 3, 10] as const).map((t) => (
              <button
                key={t}
                onClick={() => setTimerSeconds(t)}
                className={`px-2 py-1 rounded-lg transition ${
                  timerSeconds === t
                    ? 'bg-[#383B4C] text-[#FFD60A] font-bold'
                    : 'text-white/50 hover:text-white'
                }`}
              >
                {t === 0 ? 'Timer Off' : `${t}s`}
              </button>
            ))}
          </div>
        </div>

        {/* Center: Iconic Apple Photo Booth / Continuity Camera Shutter Button + Effects Button */}
        <div className="flex items-center gap-5">
          {/* 3x3 Photo Booth Effects Button */}
          <button
            onClick={() => setShowFilterGrid((g) => !g)}
            className={`flex items-center gap-2 px-4 py-2 rounded-full border text-[12px] font-medium transition ${
              showFilterGrid
                ? 'bg-[#0A84FF] border-[#0A84FF] text-white shadow-lg'
                : 'bg-[#252733] border-white/15 text-white/85 hover:bg-[#303342]'
            }`}
          >
            <Sparkles
              className={`w-4 h-4 fill-current ${
                showFilterGrid ? 'text-white' : 'text-[#0A84FF]'
              }`}
            />
            <span>Effects</span>
          </button>

          {/* Apple Concentric Metallic Shutter Button */}
          <button
            onClick={handleShutterTrigger}
            title={captureMode === 'video' ? 'Start/Stop Recording' : 'Take Photo'}
            className="w-14 h-14 rounded-full p-1 border-[3px] border-white/85 flex items-center justify-center transition-transform active:scale-90 hover:border-white shadow-[0_0_25px_rgba(255,255,255,0.2)]"
          >
            <div
              className={`transition-all duration-200 ${
                captureMode === 'video'
                  ? isRecording
                    ? 'w-6 h-6 rounded-md bg-[#FF453A]'
                    : 'w-11 h-11 rounded-full bg-[#FF453A]'
                  : 'w-11 h-11 rounded-full bg-white hover:bg-white/90 flex items-center justify-center'
              }`}
            >
              {captureMode !== 'video' && (
                <Camera className="w-5 h-5 text-black/75 stroke-[2.2]" />
              )}
            </div>
          </button>

          {/* Quick Ring Light Brightness Pill */}
          <button
            onClick={handleToggleRingLight}
            className={`flex items-center gap-2 px-4 py-2 rounded-full border text-[12px] font-medium transition ${
              studioEffects.edgeLight.enabled
                ? 'bg-amber-400 text-black border-amber-300 font-semibold shadow-[0_0_20px_rgba(251,191,36,0.4)]'
                : 'bg-[#252733] border-white/15 text-white/85 hover:bg-[#303342]'
            }`}
          >
            <Sun className="w-4 h-4" />
            <span>{studioEffects.edgeLight.enabled ? 'Light On' : 'Ring Light'}</span>
          </button>
        </div>

        {/* Right: Filmstrip Thumbnail Tray */}
        <div className="flex items-center gap-2">
          {shots.slice(0, 4).map((shot) => (
            <button
              key={shot.id}
              onClick={() => setSelectedShot(shot)}
              className="group relative w-14 h-10 rounded-lg overflow-hidden border border-white/20 bg-[#222530] hover:border-[#0A84FF] transition shrink-0"
              title={`${shot.filterName} · ${shot.timestamp}`}
            >
              {shot.dataUrl ? (
                <img
                  src={shot.dataUrl}
                  alt={shot.filterName}
                  className="w-full h-full object-cover"
                />
              ) : (
                <div className="w-full h-full flex items-center justify-center bg-gradient-to-br from-[#2b3244] to-[#161922] text-[9px] font-mono text-white/60">
                  RAW
                </div>
              )}
            </button>
          ))}
        </div>
      </footer>

      {/* =====================================================================
          4. LIGHTBOX MODAL FOR CAPTURED PHOTO INSPECTION & EXPORT
      ====================================================================== */}
      {selectedShot && (
        <div
          onClick={() => setSelectedShot(null)}
          className="fixed inset-0 z-50 bg-black/80 backdrop-blur-xl flex items-center justify-center p-6"
        >
          <div
            onClick={(e) => e.stopPropagation()}
            className="bg-[#1C1D24] border border-white/15 rounded-2xl max-w-2xl w-full overflow-hidden shadow-2xl"
          >
            <div className="px-4 py-3 border-b border-white/10 flex items-center justify-between">
              <div className="flex items-center gap-2">
                <span className="text-[13px] font-semibold text-white">
                  Captured Frame · {selectedShot.filterName}
                </span>
                <span className="text-[11px] font-mono text-white/45">
                  {selectedShot.timestamp}
                </span>
              </div>
              <button
                onClick={() => setSelectedShot(null)}
                className="p-1 rounded-lg text-white/60 hover:text-white hover:bg-white/10"
              >
                <X className="w-4 h-4" />
              </button>
            </div>
            <div className="p-4 bg-black flex items-center justify-center">
              {selectedShot.dataUrl ? (
                <img
                  src={selectedShot.dataUrl}
                  alt={selectedShot.filterName}
                  className="max-h-[420px] rounded-xl border border-white/10"
                />
              ) : (
                <div className="h-64 w-full flex items-center justify-center text-white/50 text-sm">
                  Click the Shutter button to capture a live high-resolution PNG frame.
                </div>
              )}
            </div>
            <div className="px-4 py-3 border-t border-white/10 flex items-center justify-between">
              <button
                onClick={() => {
                  setShots((prev) => prev.filter((s) => s.id !== selectedShot.id));
                  setSelectedShot(null);
                }}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-[#FF453A]/15 text-[#FF453A] text-xs font-medium hover:bg-[#FF453A]/25"
              >
                <Trash2 className="w-3.5 h-3.5" />
                <span>Delete Photo</span>
              </button>
              {selectedShot.dataUrl && (
                <a
                  href={selectedShot.dataUrl}
                  download={`CamDroid-${selectedShot.filterName}-${Date.now()}.png`}
                  className="flex items-center gap-1.5 px-4 py-1.5 rounded-lg bg-[#0A84FF] text-white text-xs font-semibold hover:brightness-110"
                >
                  <Download className="w-3.5 h-3.5" />
                  <span>Save PNG</span>
                </a>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default App;
