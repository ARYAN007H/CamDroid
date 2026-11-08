use super::v2::{
    MediaFlags, MediaKind, MediaPacket, MediaTimestamp, SessionId, V2_HEADER_LEN, V2_MAGIC,
    V2_MAX_PAYLOAD_SIZE, V2_VERSION,
};

fn timestamped_video_packet() -> MediaPacket {
    MediaPacket::new(
        MediaKind::Video,
        MediaFlags::KEY_FRAME | MediaFlags::CONFIGURATION,
        SessionId(0x0807_0605_0403_0201),
        0x100f_0e0d_0c0b_0a09,
        MediaTimestamp(0x1817_1615_1413_1211),
        vec![0x00, 0x00, 0x00, 0x01, 0x65],
    )
}

#[test]
fn v2_timestamped_video_packet_round_trips_with_its_identity_fields() {
    // A regression in byte order, sequence assignment, or timestamps must not
    // silently turn an encoded frame into a different media unit.
    let packet = timestamped_video_packet();

    let encoded = packet.encode().unwrap();
    let decoded = MediaPacket::decode(&encoded).unwrap();

    assert_eq!(decoded, packet);
}

#[test]
fn v2_encoding_matches_the_shared_little_endian_test_vector() {
    // This literal is mirrored by ProtocolV2Test on Android. Reordering any
    // header field or changing its width must break interoperability here.
    let encoded = timestamped_video_packet().encode().unwrap();

    let expected = vec![
        0x43, 0x4d, 0x44, 0x32, // "CMD2"
        0x02, // protocol version
        0x01, // video
        0x03, 0x00, // key frame | configuration
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, // session
        0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10, // sequence
        0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, // capture time
        0x05, 0x00, 0x00, 0x00, // payload length
        0x00, 0x00, 0x00, 0x01, 0x65,
    ];

    assert_eq!(V2_MAGIC, *b"CMD2");
    assert_eq!(V2_VERSION, 2);
    assert_eq!(V2_HEADER_LEN, 36);
    assert_eq!(encoded, expected);
}

#[test]
fn v2_rejects_a_declared_payload_larger_than_the_safety_limit() {
    // An attacker-controlled length must never cause a receiver allocation.
    let mut bytes = vec![0; V2_HEADER_LEN];
    bytes[..4].copy_from_slice(&V2_MAGIC);
    bytes[4] = V2_VERSION;
    bytes[5] = MediaKind::Video as u8;
    bytes[32..36].copy_from_slice(&(V2_MAX_PAYLOAD_SIZE as u32 + 1).to_le_bytes());

    let error = MediaPacket::decode(&bytes).unwrap_err();

    assert!(error.to_string().contains("too large"));
}

#[test]
fn v2_rejects_truncated_payloads() {
    // A partial TCP read may contain a complete envelope but not its payload.
    let mut bytes = timestamped_video_packet().encode().unwrap();
    bytes.pop();

    let error = MediaPacket::decode(&bytes).unwrap_err();

    assert!(error.to_string().contains("truncated"));
}

#[test]
fn v2_rejects_an_unsupported_version_before_dispatching_media() {
    // Sending old/new protocol traffic into a v2 session is a compatibility
    // failure, not a best-effort decode.
    let mut bytes = timestamped_video_packet().encode().unwrap();
    bytes[4] = 3;

    let error = MediaPacket::decode(&bytes).unwrap_err();

    assert!(error.to_string().contains("unsupported protocol version"));
}
