use super::*;
use super::super::dave::VoiceDaveOutboundPayload;
use super::super::rtp::{build_voice_rtp_packet, build_voice_rtp_packet_with_marker};
use ::opus::{Channels, Decoder as OpusDecoder, SampleRate as OpusSampleRate};
use aes_gcm::{
    Aes256Gcm, Nonce as AesGcmNonce,
    aead::{Aead, KeyInit, Payload},
};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

#[test]
fn rtp_header_parses_minimal_and_extended_packets() {
    let packet = [
        0x80, 0x78, 0x12, 0x34, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
    ];

    let header = parse_rtp_header(&packet).expect("RTP header should parse");

    assert_eq!(
        header,
        RtpHeader {
            has_padding: false,
            marker: false,
            payload_type: 0x78,
            sequence: 0x1234,
            timestamp: 0x01020304,
            ssrc: 0x05060708,
            authenticated_header_len: 12,
            encrypted_extension_body_len: 0,
            payload_offset: 12,
        }
    );

    let mut extended = vec![0x91, 0x78, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1];
    extended.extend_from_slice(&0x11223344u32.to_be_bytes());
    extended.extend_from_slice(&0x1000u16.to_be_bytes());
    extended.extend_from_slice(&1u16.to_be_bytes());
    extended.extend_from_slice(&0x55667788u32.to_be_bytes());

    let header = parse_rtp_header(&extended).expect("extended RTP header should parse");

    assert_eq!(header.authenticated_header_len, 20);
    assert_eq!(header.encrypted_extension_body_len, 4);
    assert_eq!(header.payload_offset, 24);
}

#[test]
fn rtp_decrypts_aead_rtpsize_modes_and_strips_extension_body_and_padding() {
    let key = [7u8; 32];
    let nonce_suffix = [1, 2, 3, 4];
    let mut header = vec![0xb0, 0x78, 0, 7, 0, 0, 0, 8, 0, 0, 0, 9];
    header.extend_from_slice(&0x1000u16.to_be_bytes());
    header.extend_from_slice(&1u16.to_be_bytes());
    let plaintext = [
        b"ext!".as_slice(),
        b"opus-frame".as_slice(),
        [0, 0, 3].as_slice(),
    ]
    .concat();

    for mode in [AEAD_AES256_GCM_RTPSIZE, AEAD_XCHACHA20_POLY1305_RTPSIZE] {
        let mut packet = header.clone();
        packet.extend(encrypt_test_rtp_payload(
            mode,
            &key,
            &header,
            &plaintext,
            nonce_suffix,
        ));
        packet.extend_from_slice(&nonce_suffix);
        let rtp_header = parse_rtp_header(&packet).expect("RTP header should parse");
        let decryptor = VoiceRtpDecryptor::new(mode, &key).expect("decryptor should build");

        assert!(rtp_header.has_padding);
        let decrypted = decryptor
            .decrypt_packet(&packet, &rtp_header)
            .expect("RTP payload should decrypt");

        assert_eq!(decrypted.encrypted_extension_body_len, 4);
        assert_eq!(decrypted.extension_profile, Some(0x1000));
        assert_eq!(decrypted.extension_body, b"ext!");
        assert_eq!(decrypted.media_payload, b"opus-frame");
    }
}

#[test]
fn outbound_rtp_packet_builder_sets_header_and_advances_state() {
    let mut state = VoiceOutboundRtpState {
        sequence: u16::MAX,
        timestamp: u32::MAX - 100,
        ssrc: 0x01020304,
    };

    let packet = state
        .packetize(&DISCORD_OPUS_SILENCE_FRAME)
        .expect("RTP packet should build");
    let header = parse_rtp_header(&packet).expect("RTP header should parse");

    assert_eq!(packet[0], 0x80);
    assert_eq!(header.payload_type, DISCORD_VOICE_PAYLOAD_TYPE);
    assert_eq!(header.sequence, u16::MAX);
    assert_eq!(header.timestamp, u32::MAX - 100);
    assert_eq!(header.ssrc, 0x01020304);
    assert_eq!(header.payload_offset, RTP_HEADER_MIN_LEN);
    assert_eq!(&packet[header.payload_offset..], DISCORD_OPUS_SILENCE_FRAME);
    assert_eq!(state.sequence, 0);
    assert_eq!(
        state.timestamp,
        (u32::MAX - 100).wrapping_add(DISCORD_OPUS_TIMESTAMP_INCREMENT)
    );

    assert_eq!(
        build_voice_rtp_packet(1, 2, 3, &[]).expect_err("empty payload should fail"),
        "voice RTP packet requires a non-empty Opus payload"
    );
}

#[test]
fn outbound_rtp_encrypts_aead_rtpsize_modes_for_decrypt_round_trip() {
    let key = [9u8; 32];
    let nonce_suffix = [4, 3, 2, 1];
    let packet =
        build_voice_rtp_packet(7, 960, 42, b"opus-frame").expect("RTP packet should build");

    for mode in [AEAD_AES256_GCM_RTPSIZE, AEAD_XCHACHA20_POLY1305_RTPSIZE] {
        let encryptor = VoiceRtpEncryptor::new(mode, &key).expect("encryptor should build");
        let encrypted = encryptor
            .encrypt_packet(&packet, nonce_suffix)
            .expect("RTP payload should encrypt");
        let header = parse_rtp_header(&encrypted).expect("encrypted RTP header should parse");
        let decryptor = VoiceRtpDecryptor::new(mode, &key).expect("decryptor should build");
        let decrypted = decryptor
            .decrypt_packet(&encrypted, &header)
            .expect("RTP payload should decrypt");

        assert_eq!(
            &encrypted[encrypted.len() - RTP_AEAD_NONCE_SUFFIX_BYTES..],
            nonce_suffix
        );
        assert_eq!(header.sequence, 7);
        assert_eq!(header.timestamp, 960);
        assert_eq!(header.ssrc, 42);
        assert_eq!(decrypted.media_payload, b"opus-frame");
    }
}

#[test]
fn opus_encoder_encodes_decodable_20ms_stereo_frame() {
    let mut encoder = VoiceOpusEncode::new().expect("Opus encoder should build");
    let pcm = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];

    let opus = encoder
        .encode_20ms_i16(&pcm)
        .expect("20 ms stereo frame should encode");

    assert!(!opus.is_empty());

    let mut decoder = OpusDecoder::new(Channels::Stereo, OpusSampleRate::Hz48000)
        .expect("Opus decoder should build");
    let mut decoded = vec![0.0f32; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    let samples_per_channel = decoder
        .decode_float_to_slice(&opus, &mut decoded, false)
        .expect("encoded Opus should decode");

    assert_eq!(samples_per_channel, DISCORD_OPUS_FRAME_SAMPLES_PER_CHANNEL);
    assert_eq!(
        encoder
            .encode_20ms_i16(&pcm[..pcm.len() - 1])
            .expect_err("short frame should fail"),
        format!(
            "voice Opus encoder expected {} interleaved stereo samples, got {}",
            DISCORD_OPUS_20MS_STEREO_SAMPLES,
            DISCORD_OPUS_20MS_STEREO_SAMPLES - 1
        )
    );
}

#[test]
fn system_audio_opus_encoder_encodes_decodable_20ms_stereo_frame() {
    let mut encoder =
        VoiceOpusEncode::new_system_audio().expect("system audio Opus encoder should build");
    let pcm = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];

    let opus = encoder
        .encode_20ms_i16(&pcm)
        .expect("system audio frame should encode");

    let mut decoder = OpusDecoder::new(Channels::Stereo, OpusSampleRate::Hz48000)
        .expect("Opus decoder should build");
    let mut decoded = vec![0.0f32; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    let samples_per_channel = decoder
        .decode_float_to_slice(&opus, &mut decoded, false)
        .expect("system audio Opus should decode");

    assert_eq!(samples_per_channel, DISCORD_OPUS_FRAME_SAMPLES_PER_CHANNEL);
}

#[test]
fn fake_outbound_noops_when_capture_gate_is_closed() {
    let mut state = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, 10);
    let rtp = state.rtp;

    assert_eq!(
        state
            .send_opus_frame(b"opus-frame")
            .expect("send should no-op"),
        VoiceOutboundSendOutcome::Noop
    );
    assert!(state.events().is_empty());
    assert_eq!(state.rtp, rtp);
    assert_eq!(state.nonce_suffix, 10);

    state.set_capture_gate(true, true);
    assert_eq!(
        state
            .send_opus_frame(b"opus-frame")
            .expect("muted send should no-op"),
        VoiceOutboundSendOutcome::Noop
    );
    assert!(state.events().is_empty());
    assert_eq!(state.rtp, rtp);
    assert_eq!(state.nonce_suffix, 10);
}

#[test]
fn fake_outbound_blocks_dave_active_plaintext_fallback() {
    let mut state = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, 10);
    state.set_capture_gate(true, false);
    state.set_dave_active(true);
    let rtp = state.rtp;

    assert_eq!(
        state
            .send_opus_frame(b"opus-frame")
            .expect("DAVE block should be reported"),
        VoiceOutboundSendOutcome::Blocked(VoiceOutboundSendBlockReason::DaveOutboundUnsupported)
    );
    assert!(state.events().is_empty());
    assert_eq!(state.rtp, rtp);
    assert_eq!(state.nonce_suffix, 10);
}

#[test]
fn fake_outbound_uses_dave_outbound_policy_before_transport_encrypt() {
    let mut dave = VoiceDaveState::new(&test_voice_gateway_session());
    let mut state = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, 30);
    state.set_capture_gate(true, false);

    assert_eq!(
        state
            .send_opus_frame_with_dave(b"opus-frame", &mut dave)
            .expect("DAVE inactive frame should send"),
        VoiceOutboundSendOutcome::Sent
    );
    assert_fake_packet(
        AEAD_AES256_GCM_RTPSIZE,
        &state.events()[1],
        7,
        960,
        b"opus-frame",
        30u32.to_be_bytes(),
        true,
    );

    let mut dave = VoiceDaveState::new(&test_voice_gateway_session());
    dave.reinit(1).expect("DAVE session should initialize");
    let mut blocked = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, 30);
    blocked.set_capture_gate(true, false);
    let rtp = blocked.rtp;

    assert_eq!(
        blocked
            .send_opus_frame_with_dave(b"opus-frame", &mut dave)
            .expect("DAVE not-ready frame should block"),
        VoiceOutboundSendOutcome::Blocked(VoiceOutboundSendBlockReason::DaveOutboundNotReady)
    );
    assert!(blocked.events().is_empty());
    assert_eq!(blocked.rtp, rtp);
    assert_eq!(blocked.nonce_suffix, 30);
}

#[test]
fn fake_outbound_sends_encrypted_packets_without_live_io() {
    for mode in [AEAD_AES256_GCM_RTPSIZE, AEAD_XCHACHA20_POLY1305_RTPSIZE] {
        let mut state = fake_outbound_state(mode, 0x01020304);
        state.set_capture_gate(true, false);

        assert_eq!(
            state
                .send_opus_frame(b"opus-frame")
                .expect("first frame should send"),
            VoiceOutboundSendOutcome::Sent
        );
        assert_eq!(state.events().len(), 2);
        assert_eq!(
            state.events()[0],
            VoiceOutboundSendEvent::Speaking {
                speaking: true,
                ssrc: 42,
            }
        );
        assert_fake_packet(
            mode,
            &state.events()[1],
            7,
            960,
            b"opus-frame",
            [1, 2, 3, 4],
            true,
        );
        assert_eq!(state.rtp.sequence, 8);
        assert_eq!(state.rtp.timestamp, 960);
        assert_eq!(state.nonce_suffix, 0x01020305);

        state.advance_media_clock_frames(1);
        assert_eq!(
            state
                .send_opus_frame(b"next-frame")
                .expect("second frame should send"),
            VoiceOutboundSendOutcome::Sent
        );
        assert_eq!(state.events().len(), 3);
        assert_fake_packet(
            mode,
            &state.events()[2],
            8,
            1920,
            b"next-frame",
            [1, 2, 3, 5],
            false,
        );
        assert_eq!(state.rtp.sequence, 9);
        assert_eq!(state.rtp.timestamp, 1920);
        assert_eq!(state.nonce_suffix, 0x01020306);
    }
}

#[test]
fn fake_outbound_media_clock_and_talkspurt_marker_are_independent() {
    let mut state = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, 10);
    state.set_capture_gate(true, false);

    state.advance_media_clock_frames(10);
    assert_eq!(state.rtp.sequence, 7);
    assert_eq!(state.rtp.timestamp, 10_560);
    assert_eq!(state.nonce_suffix, 10);

    assert_eq!(
        state
            .send_opus_frame(b"first-talkspurt")
            .expect("first talkspurt should send"),
        VoiceOutboundSendOutcome::Sent
    );
    let first_header = fake_packet_header(&state.events()[1]);
    assert!(first_header.marker);
    assert_eq!(first_header.timestamp, 10_560);
    assert_eq!(state.rtp.sequence, 8);
    assert_eq!(state.rtp.timestamp, 10_560);
    assert_eq!(state.nonce_suffix, 11);

    state.advance_media_clock_frames(1);
    assert_eq!(
        state
            .send_opus_frame(b"same-talkspurt")
            .expect("continued talkspurt should send"),
        VoiceOutboundSendOutcome::Sent
    );
    let continued_header = fake_packet_header(&state.events()[2]);
    assert!(!continued_header.marker);
    assert_eq!(continued_header.timestamp, 11_520);

    assert_eq!(
        state.stop_speaking().expect("talkspurt should stop"),
        VoiceOutboundSendOutcome::Sent
    );
    state.advance_media_clock_frames(50);
    assert_eq!(
        state
            .send_opus_frame(b"next-talkspurt")
            .expect("next talkspurt should send"),
        VoiceOutboundSendOutcome::Sent
    );
    let next_header = fake_packet_header(
        state
            .events()
            .last()
            .expect("next talkspurt should queue a packet"),
    );
    assert!(next_header.marker);
    assert_eq!(next_header.timestamp, 59_520);
}

#[test]
fn trailing_silence_is_paced_and_can_be_cancelled() {
    let mut tail = VoiceTrailingSilence::default();

    tail.start(true);
    for index in 0..DISCORD_TRAILING_SILENCE_FRAMES {
        assert_eq!(
            tail.take_frame(),
            Some(index + 1 == DISCORD_TRAILING_SILENCE_FRAMES)
        );
    }
    assert_eq!(tail.take_frame(), None);

    tail.start(true);
    assert_eq!(tail.take_frame(), Some(false));
    tail.cancel();
    assert_eq!(tail.take_frame(), None);

    tail.start(false);
    assert_eq!(tail.take_frame(), None);
}

#[test]
fn fake_outbound_sends_trailing_silence_when_stopping_speech() {
    let mut state = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, 20);
    state.set_capture_gate(true, false);

    assert_eq!(
        state
            .send_opus_frame(b"speech")
            .expect("speech frame should send"),
        VoiceOutboundSendOutcome::Sent
    );
    assert_eq!(
        state.stop_speaking().expect("stop speaking should send"),
        VoiceOutboundSendOutcome::Sent
    );
    assert_eq!(
        state.events(),
        &[
            VoiceOutboundSendEvent::Speaking {
                speaking: true,
                ssrc: 42,
            },
            VoiceOutboundSendEvent::Packet {
                bytes: fake_packet_bytes(AEAD_AES256_GCM_RTPSIZE, 7, 960, 42, b"speech", [0, 0, 0, 20], true),
            },
            VoiceOutboundSendEvent::Speaking {
                speaking: false,
                ssrc: 42,
            },
        ]
    );
    assert_eq!(state.rtp.sequence, 8);
    assert_eq!(state.rtp.timestamp, 960);
    assert_eq!(state.nonce_suffix, 21);
    assert!(!state.speaking);
}

#[test]
fn fake_outbound_nonce_exhaustion_fails_without_state_change() {
    let mut state = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, u32::MAX);
    state.set_capture_gate(true, false);
    let rtp = state.rtp;

    assert_eq!(
        state
            .send_opus_frame(b"opus-frame")
            .expect_err("exhausted nonce should fail"),
        "voice RTP nonce suffix exhausted"
    );
    assert!(state.events().is_empty());
    assert_eq!(state.rtp, rtp);
    assert_eq!(state.nonce_suffix, u32::MAX);

    let mut stopping = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, u32::MAX - 2);
    stopping.set_capture_gate(true, false);
    stopping.speaking = true;
    let rtp = stopping.rtp;
    assert_eq!(
        stopping
            .stop_speaking()
            .expect("stop should still clear speaking"),
        VoiceOutboundSendOutcome::Sent
    );
    assert_eq!(
        stopping.events(),
        &[VoiceOutboundSendEvent::Speaking {
            speaking: false,
            ssrc: 42,
        }]
    );
    assert_eq!(stopping.rtp, rtp);
    assert_eq!(stopping.nonce_suffix, u32::MAX - 2);
    assert!(!stopping.speaking);
}

#[test]
fn rtp_header_rejects_malformed_packets() {
    assert_eq!(
        parse_rtp_header(&[0; 11]).expect_err("short packet should fail"),
        "RTP packet is too short"
    );

    let packet = [0x40, 0x78, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1];

    assert_eq!(
        parse_rtp_header(&packet).expect_err("wrong version should fail"),
        "RTP packet has unsupported version"
    );
}

#[test]
fn rtp_header_rejects_rtcp_reports_before_payload_type_masking() {
    let local_ssrc = 0x0000_f5e7u32;
    let mut receiver_report = vec![0x80, 0xc9, 0, 7];
    receiver_report.extend_from_slice(&local_ssrc.to_be_bytes());
    receiver_report.extend_from_slice(&[0, 0, 0, 0]);

    assert!(looks_like_rtcp_packet(&receiver_report));
    assert_eq!(rtcp_sender_ssrc(&receiver_report), Some(local_ssrc));
    assert_eq!(
        parse_rtp_header(&receiver_report).expect_err("RTCP should not parse as RTP"),
        "RTP parser received RTCP packet"
    );

    let sender_report = [0x80, 0xc8, 0, 12, 0, 0, 0xf5, 0xe7, 0, 0, 0, 0];
    assert!(looks_like_rtcp_packet(&sender_report));
    assert_eq!(
        parse_rtp_header(&sender_report).expect_err("RTCP should not parse as RTP"),
        "RTP parser received RTCP packet"
    );
}

fn fake_outbound_state(mode: &str, nonce_suffix: u32) -> VoiceOutboundSendState {
    VoiceOutboundSendState::new(
        mode,
        &[9u8; 32],
        VoiceOutboundRtpState {
            sequence: 7,
            timestamp: 960,
            ssrc: 42,
        },
        nonce_suffix,
    )
    .expect("fake outbound state should build")
}

pub(super) fn test_voice_gateway_session() -> VoiceGatewaySession {
    VoiceGatewaySession {
        connection_id: 0,
        scope: VoiceScope::Guild(Id::new(1)),
        channel_id: Id::new(10),
        user_id: Id::new(20),
        session_id: "voice-session".to_owned(),
        endpoint: "voice.example.com".to_owned(),
        token: "voice-token".to_owned(),
    }
}

fn assert_fake_packet(
    mode: &str,
    event: &VoiceOutboundSendEvent,
    sequence: u16,
    timestamp: u32,
    expected_payload: &[u8],
    nonce_suffix: [u8; RTP_AEAD_NONCE_SUFFIX_BYTES],
    marker: bool,
) {
    let VoiceOutboundSendEvent::Packet { bytes } = event else {
        panic!("expected fake packet event, got {event:?}");
    };
    let packet_bytes = bytes.as_slice();
    let header = parse_rtp_header(packet_bytes).expect("fake RTP header should parse");
    let decryptor = VoiceRtpDecryptor::new(mode, &[9u8; 32]).expect("decryptor should build");
    let decrypted = decryptor
        .decrypt_packet(packet_bytes, &header)
        .expect("fake RTP packet should decrypt");

    let actual_nonce_suffix = &packet_bytes[packet_bytes.len() - RTP_AEAD_NONCE_SUFFIX_BYTES..];
    assert_eq!(actual_nonce_suffix, &nonce_suffix[..]);
    assert_eq!(header.marker, marker);
    assert_eq!(header.sequence, sequence);
    assert_eq!(header.timestamp, timestamp);
    assert_eq!(header.ssrc, 42);
    assert_eq!(decrypted.media_payload, expected_payload);
}

fn fake_packet_header(event: &VoiceOutboundSendEvent) -> RtpHeader {
    let VoiceOutboundSendEvent::Packet { bytes } = event else {
        panic!("expected fake packet event, got {event:?}");
    };
    parse_rtp_header(bytes).expect("fake RTP header should parse")
}

fn encrypt_test_rtp_payload(
    mode: &str,
    key: &[u8],
    aad: &[u8],
    plaintext: &[u8],
    nonce_suffix: [u8; RTP_AEAD_NONCE_SUFFIX_BYTES],
) -> Vec<u8> {
    match mode {
        AEAD_AES256_GCM_RTPSIZE => {
            let cipher = Aes256Gcm::new_from_slice(key).expect("test key is valid");
            let mut nonce = [0u8; 12];
            nonce[..RTP_AEAD_NONCE_SUFFIX_BYTES].copy_from_slice(&nonce_suffix);
            cipher
                .encrypt(
                    AesGcmNonce::from_slice(&nonce),
                    Payload {
                        msg: plaintext,
                        aad,
                    },
                )
                .expect("test payload encrypts")
        }
        AEAD_XCHACHA20_POLY1305_RTPSIZE => {
            let cipher = XChaCha20Poly1305::new_from_slice(key).expect("test key is valid");
            let mut nonce = [0u8; 24];
            nonce[..RTP_AEAD_NONCE_SUFFIX_BYTES].copy_from_slice(&nonce_suffix);
            cipher
                .encrypt(
                    XNonce::from_slice(&nonce),
                    Payload {
                        msg: plaintext,
                        aad,
                    },
                )
                .expect("test payload encrypts")
        }
        other => panic!("unsupported test mode: {other}"),
    }
}

fn fake_packet_bytes(
    mode: &str,
    sequence: u16,
    timestamp: u32,
    ssrc: u32,
    opus_payload: &[u8],
    nonce_suffix: [u8; RTP_AEAD_NONCE_SUFFIX_BYTES],
    marker: bool,
) -> Vec<u8> {
    let packet = build_voice_rtp_packet_with_marker(sequence, timestamp, ssrc, marker, opus_payload)
        .expect("fake RTP packet should build");
    let encryptor = VoiceRtpEncryptor::new(mode, &[9u8; 32]).expect("encryptor should build");
    encryptor
        .encrypt_packet(&packet, nonce_suffix)
        .expect("fake RTP packet should encrypt")
}
