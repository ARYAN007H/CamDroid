use crate::protocol::v2::{SessionId, StreamConfig};
use crate::session::{
    CapabilitySet, SessionEffect, SessionErrorCode, SessionEvent, SessionMachine, SessionMessage,
    SessionPhase, StreamRequest, VideoMode,
};

fn supported_capabilities() -> CapabilitySet {
    CapabilitySet {
        protocol_versions: vec![2],
        codecs: vec!["h264".to_string()],
        video_modes: vec![VideoMode {
            width: 1920,
            height: 1080,
            fps: 30,
            min_bitrate_bps: 1_000_000,
            max_bitrate_bps: 12_000_000,
        }],
        audio_codecs: vec!["aac".to_string()],
    }
}

fn h264_request() -> StreamRequest {
    StreamRequest {
        config: StreamConfig {
            codec: "h264".to_string(),
            width: 1920,
            height: 1080,
            fps: 30,
            bitrate_bps: 8_000_000,
            profile: Some("baseline".to_string()),
            level: None,
            audio: None,
        },
        audio_enabled: true,
    }
}

fn negotiated_stream(session_id: u64, request: &StreamRequest) -> crate::session::NegotiatedStream {
    crate::session::NegotiatedStream {
        session_id: SessionId(session_id),
        config: request.config.clone(),
        audio_enabled: request.audio_enabled,
    }
}

fn negotiating_session() -> SessionMachine {
    let mut session = SessionMachine::new();
    session.apply(SessionEvent::ConnectRequested).unwrap();
    session.apply(SessionEvent::TransportConnected).unwrap();
    session
        .apply(SessionEvent::CapabilitiesReceived(supported_capabilities()))
        .unwrap();
    session
}

#[test]
fn session_moves_from_idle_to_streaming_only_after_an_acknowledgement() {
    // Starting a decoder before the peer accepts its selected mode would let
    // the desktop present an unverified configuration as active.
    let mut session = negotiating_session();
    assert_eq!(session.phase(), &SessionPhase::Negotiating);

    let request = h264_request();
    let effects = session
        .apply(SessionEvent::StartRequested(request.clone()))
        .unwrap();
    assert_eq!(session.phase(), &SessionPhase::AwaitingStartAck);
    assert_eq!(effects, vec![SessionEffect::SendStart(request.clone())]);

    let stream = negotiated_stream(1, &request);
    let effects = session
        .apply(SessionEvent::StartAcknowledged(stream.clone()))
        .unwrap();

    assert_eq!(session.phase(), &SessionPhase::Streaming(stream.clone()));
    assert_eq!(effects, vec![SessionEffect::StartMedia(stream)]);
}

#[test]
fn session_rejects_an_acknowledgement_without_a_pending_start() {
    // A stale acknowledgement from an old connection must not resurrect a
    // stopped media pipeline.
    let mut session = SessionMachine::new();
    let stream = crate::session::NegotiatedStream {
        session_id: SessionId(9),
        config: h264_request().config,
        audio_enabled: true,
    };

    let error = session
        .apply(SessionEvent::StartAcknowledged(stream))
        .unwrap_err();

    assert_eq!(error.code, SessionErrorCode::InvalidTransition);
    assert_eq!(session.phase(), &SessionPhase::Idle);
}

#[test]
fn session_rejects_a_request_outside_the_advertised_capabilities() {
    // A codec that the phone did not advertise must never reach its encoder.
    let mut session = negotiating_session();
    let mut request = h264_request();
    request.config.codec = "hevc".to_string();

    let error = session
        .apply(SessionEvent::StartRequested(request))
        .unwrap_err();

    assert_eq!(error.code, SessionErrorCode::UnsupportedMode);
    assert_eq!(session.phase(), &SessionPhase::Negotiating);
}

#[test]
fn reconnecting_clears_media_queues_before_a_new_handshake() {
    // Old decoded frames must not be emitted after a peer reconnects.
    let mut session = negotiating_session();
    let request = h264_request();
    session
        .apply(SessionEvent::StartRequested(request.clone()))
        .unwrap();
    let stream = negotiated_stream(1, &request);
    session
        .apply(SessionEvent::StartAcknowledged(stream))
        .unwrap();

    let effects = session.apply(SessionEvent::ConnectionLost).unwrap();

    assert_eq!(session.phase(), &SessionPhase::Reconnecting);
    assert_eq!(effects, vec![SessionEffect::ClearMediaQueues]);
    assert_eq!(session.queue_generation(), 1);
}

#[test]
fn reconfiguration_waits_for_an_acknowledgement_before_replacing_the_active_mode() {
    // Decoder/output reset is safe only after the peer confirms the new mode.
    let mut session = negotiating_session();
    let request = h264_request();
    session
        .apply(SessionEvent::StartRequested(request.clone()))
        .unwrap();
    let original = negotiated_stream(1, &request);
    session
        .apply(SessionEvent::StartAcknowledged(original.clone()))
        .unwrap();

    let mut changed_request = h264_request();
    changed_request.config.bitrate_bps = 4_000_000;
    let effects = session
        .apply(SessionEvent::ReconfigureRequested(changed_request))
        .unwrap();
    assert_eq!(session.phase(), &SessionPhase::AwaitingReconfigureAck);
    assert_eq!(effects.len(), 1);

    let changed = crate::session::NegotiatedStream {
        session_id: SessionId(10),
        config: StreamConfig {
            bitrate_bps: 4_000_000,
            ..original.config
        },
        audio_enabled: true,
    };
    let effects = session
        .apply(SessionEvent::ReconfigureAcknowledged(changed.clone()))
        .unwrap();

    assert_eq!(session.phase(), &SessionPhase::Streaming(changed.clone()));
    assert_eq!(
        effects,
        vec![
            SessionEffect::ClearMediaQueues,
            SessionEffect::ReconfigureMedia(changed),
        ]
    );
    assert_eq!(session.queue_generation(), 1);
}

#[test]
fn negotiation_messages_preserve_the_selected_stream_across_json() {
    // Transport code must not reinterpret a server acknowledgement as a
    // different mode before the media pipeline is allowed to start.
    let request = h264_request();
    let message = SessionMessage::StreamAccepted(negotiated_stream(7, &request));

    let json = serde_json::to_vec(&message).unwrap();
    let decoded: SessionMessage = serde_json::from_slice(&json).unwrap();

    assert_eq!(decoded, message);
}

fn phone_with_modes(modes: Vec<VideoMode>, codecs: &[&str], audio: bool) -> CapabilitySet {
    CapabilitySet {
        protocol_versions: vec![2],
        codecs: codecs.iter().map(|codec| codec.to_string()).collect(),
        video_modes: modes,
        audio_codecs: if audio {
            vec!["aac".to_string()]
        } else {
            vec![]
        },
    }
}

fn mode(width: u32, height: u32, fps: u32) -> VideoMode {
    VideoMode {
        width,
        height,
        fps,
        min_bitrate_bps: 1_000_000,
        max_bitrate_bps: 50_000_000,
    }
}

fn desired(codec: &str, width: u32, height: u32, fps: u32) -> StreamRequest {
    let mut request = h264_request();
    request.config.codec = codec.to_string();
    request.config.width = width;
    request.config.height = height;
    request.config.fps = fps;
    request.config.bitrate_bps = 8_000_000;
    request
}

#[test]
fn resolve_keeps_an_exactly_supported_request_unchanged() {
    let phone = phone_with_modes(vec![mode(1920, 1080, 60)], &["h264"], true);
    let resolved = phone.resolve(&desired("h264", 1920, 1080, 60)).unwrap();

    assert_eq!(resolved.config.bitrate_bps, 8_000_000);
    assert!(phone.supports(&resolved).is_ok());
}

#[test]
fn resolve_falls_back_to_the_only_mode_the_phone_advertises() {
    // The phone is configured for 1440p30 H.265; the desktop asked for 1080p60 H.264.
    let phone = phone_with_modes(vec![mode(2560, 1440, 30)], &["h265"], true);
    let resolved = phone.resolve(&desired("h264", 1920, 1080, 60)).unwrap();

    assert_eq!(resolved.config.codec, "h265");
    assert_eq!(
        (
            resolved.config.width,
            resolved.config.height,
            resolved.config.fps
        ),
        (2560, 1440, 30)
    );
    assert!(phone.supports(&resolved).is_ok());
}

#[test]
fn resolve_prefers_the_closest_resolution_then_frame_rate() {
    let phone = phone_with_modes(
        vec![
            mode(1280, 720, 60),
            mode(1920, 1080, 30),
            mode(3840, 2160, 30),
        ],
        &["h264"],
        true,
    );

    let resolved = phone.resolve(&desired("h264", 1920, 1080, 60)).unwrap();
    assert_eq!((resolved.config.width, resolved.config.fps), (1920, 30));
}

#[test]
fn resolve_prefers_h264_when_the_requested_codec_is_missing() {
    let phone = phone_with_modes(vec![mode(1920, 1080, 60)], &["mjpeg", "h264"], true);
    let resolved = phone.resolve(&desired("h265", 1920, 1080, 60)).unwrap();

    assert_eq!(resolved.config.codec, "h264");
}

#[test]
fn resolve_keeps_the_bitrate_inside_the_advertised_range() {
    let mut narrow = mode(1920, 1080, 60);
    narrow.min_bitrate_bps = 2_000_000;
    narrow.max_bitrate_bps = 6_000_000;
    let phone = phone_with_modes(vec![narrow], &["h264"], true);

    let resolved = phone.resolve(&desired("h264", 1920, 1080, 60)).unwrap();
    assert_eq!(resolved.config.bitrate_bps, 6_000_000);
    assert!(phone.supports(&resolved).is_ok());
}

#[test]
fn resolve_turns_audio_off_when_the_phone_has_no_audio_codec() {
    let phone = phone_with_modes(vec![mode(1920, 1080, 60)], &["h264"], false);
    let resolved = phone.resolve(&desired("h264", 1920, 1080, 60)).unwrap();

    assert!(!resolved.audio_enabled);
    assert!(phone.supports(&resolved).is_ok());
}

#[test]
fn resolve_reports_a_phone_with_nothing_to_offer() {
    let no_modes = phone_with_modes(vec![], &["h264"], true);
    assert_eq!(
        no_modes
            .resolve(&desired("h264", 1920, 1080, 60))
            .unwrap_err()
            .code,
        SessionErrorCode::UnsupportedMode
    );

    let no_codecs = phone_with_modes(vec![mode(1920, 1080, 60)], &[], true);
    assert_eq!(
        no_codecs
            .resolve(&desired("h264", 1920, 1080, 60))
            .unwrap_err()
            .code,
        SessionErrorCode::UnsupportedMode
    );

    let mut v1_only = phone_with_modes(vec![mode(1920, 1080, 60)], &["h264"], true);
    v1_only.protocol_versions = vec![1];
    assert_eq!(
        v1_only
            .resolve(&desired("h264", 1920, 1080, 60))
            .unwrap_err()
            .code,
        SessionErrorCode::UnsupportedVersion
    );
}

#[test]
fn recommended_bitrate_scales_with_pixels_frame_rate_and_codec() {
    use crate::session::recommended_bitrate;

    let h264_1080p30 = recommended_bitrate(1920, 1080, 30, "h264");
    assert_eq!(
        recommended_bitrate(1920, 1080, 60, "h264"),
        h264_1080p30 * 2
    );
    assert!(recommended_bitrate(1920, 1080, 30, "h265") < h264_1080p30);
    assert!(recommended_bitrate(1920, 1080, 30, "mjpeg") > h264_1080p30);
    assert!((5_000_000..8_000_000).contains(&h264_1080p30));
}
