//! Wi-Fi Display (Miracast) RTP + MPEG-TS (188-byte) Demuxer.
//!
//! Demultiplexes UDP RTP packets (payload type 33 MP2T) or raw UDP MPEG-TS packets
//! into H.264 (`0x1b`) / H.265 (`0x24`) Annex-B video access units and
//! LPCM (`0x83`) / AAC (`0x0f`) audio frames, feeding them directly into `Sink`.

use crate::airplay::clock::monotonic_ns;
use crate::airplay::{AudioCodec, AudioFormat, AudioPacket, Codec, Sink, VideoFrame};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const TS_PACKET_SIZE: usize = 188;
const TS_SYNC_BYTE: u8 = 0x47;

const PID_PAT: u16 = 0x0000;
const PID_DEFAULT_VIDEO: u16 = 0x1011;
const PID_DEFAULT_AUDIO: u16 = 0x1100;

const STREAM_TYPE_MPEG1_AUDIO: u8 = 0x03;
const STREAM_TYPE_MPEG2_AUDIO: u8 = 0x04;
const STREAM_TYPE_ADTS_AAC: u8 = 0x0f;
const STREAM_TYPE_LATM_AAC: u8 = 0x11;
const STREAM_TYPE_H264: u8 = 0x1b;
const STREAM_TYPE_H265: u8 = 0x24;
const STREAM_TYPE_WFD_LPCM: u8 = 0x83;

/// Elementary stream state tracked by the TS demuxer.
pub struct TsDemuxer {
    pmt_pid: Option<u16>,
    video_pid: u16,
    video_stream_type: u8,
    audio_pid: u16,
    audio_stream_type: u8,

    video_pes: Vec<u8>,
    video_pes_expected_len: usize,
    audio_pes: Vec<u8>,
    audio_pes_expected_len: usize,

    // Video codec configuration state
    configured_sps: Vec<u8>,
    configured_pps: Vec<u8>,
    configured_vps: Vec<u8>,
    video_width: u32,
    video_height: u32,
    video_configured: bool,
    need_idr_request: bool,
    frames_since_sps: u64,

    // Audio configuration state
    audio_configured: bool,
    audio_seq: u16,
    audio_rtp_ts: u32,
}

impl TsDemuxer {
    pub fn new(initial_width: u32, initial_height: u32) -> Self {
        Self {
            pmt_pid: None,
            video_pid: PID_DEFAULT_VIDEO,
            video_stream_type: STREAM_TYPE_H264,
            audio_pid: PID_DEFAULT_AUDIO,
            audio_stream_type: STREAM_TYPE_WFD_LPCM,
            video_pes: Vec::with_capacity(256 * 1024),
            video_pes_expected_len: 0,
            audio_pes: Vec::with_capacity(16 * 1024),
            audio_pes_expected_len: 0,
            configured_sps: Vec::new(),
            configured_pps: Vec::new(),
            configured_vps: Vec::new(),
            video_width: initial_width.max(1280),
            video_height: initial_height.max(720),
            video_configured: false,
            need_idr_request: false,
            frames_since_sps: 0,
            audio_configured: false,
            audio_seq: 0,
            audio_rtp_ts: 0,
        }
    }

    pub fn take_idr_request(&mut self) -> bool {
        let v = self.need_idr_request;
        self.need_idr_request = false;
        v
    }

    /// Process a single UDP datagram (either RTP-encapsulated MPEG-TS or raw MPEG-TS).
    pub fn push_udp_packet(&mut self, pkt: &[u8], sink: &dyn Sink) {
        if pkt.len() < TS_PACKET_SIZE {
            return;
        }

        let (ts_slice, rtp_marker) = if pkt[0] == TS_SYNC_BYTE {
            (pkt, false)
        } else if (pkt[0] >> 6) == 2 && pkt.len() > 12 {
            // RTP v2 header
            let csrc_count = (pkt[0] & 0x0f) as usize;
            let has_ext = (pkt[0] & 0x10) != 0;
            let has_padding = (pkt[0] & 0x20) != 0;
            let marker = (pkt[1] & 0x80) != 0;
            let mut offset = 12 + csrc_count * 4;
            if offset > pkt.len() {
                return;
            }
            if has_ext {
                if offset + 4 > pkt.len() {
                    return;
                }
                let ext_words = u16::from_be_bytes([pkt[offset + 2], pkt[offset + 3]]) as usize;
                offset += 4 + ext_words * 4;
            }
            let mut end = pkt.len();
            if has_padding && end > offset {
                let pad_len = pkt[end - 1] as usize;
                if pad_len > 0 && pad_len <= end - offset {
                    end -= pad_len;
                }
            }
            if offset >= end {
                return;
            }
            (&pkt[offset..end], marker)
        } else {
            return;
        };

        let mut pos = 0;
        while pos + TS_PACKET_SIZE <= ts_slice.len() {
            if ts_slice[pos] != TS_SYNC_BYTE {
                pos += 1;
                continue;
            }
            self.process_ts_packet(&ts_slice[pos..pos + TS_PACKET_SIZE], sink);
            pos += TS_PACKET_SIZE;
        }

        // If RTP Marker bit is set on the UDP packet and we have an unbounded video PES
        // packet containing a complete access unit, flush it immediately for minimum latency.
        if rtp_marker && self.video_pes_expected_len == 0 && self.video_pes.len() > 14 {
            if pes_has_vcl_nal(&self.video_pes, self.video_stream_type) {
                self.flush_video_pes(sink);
            }
        }
    }

    fn process_ts_packet(&mut self, ts: &[u8], sink: &dyn Sink) {
        let tei = (ts[1] & 0x80) != 0;
        if tei {
            return;
        }
        let pusi = (ts[1] & 0x40) != 0;
        let pid = (((ts[1] & 0x1f) as u16) << 8) | (ts[2] as u16);
        let afc = (ts[3] >> 4) & 0x03;

        let mut payload_offset = 4usize;
        if afc == 2 {
            return;
        } else if afc == 3 {
            if payload_offset >= TS_PACKET_SIZE {
                return;
            }
            let af_len = ts[4] as usize;
            payload_offset += 1 + af_len;
        }
        if payload_offset >= TS_PACKET_SIZE {
            return;
        }
        let payload = &ts[payload_offset..TS_PACKET_SIZE];

        if pid == PID_PAT {
            self.parse_pat(payload, pusi);
        } else if Some(pid) == self.pmt_pid {
            self.parse_pmt(payload, pusi);
        } else if pid == self.video_pid {
            if pusi {
                if !self.video_pes.is_empty() {
                    self.flush_video_pes(sink);
                }
                self.video_pes.extend_from_slice(payload);
                self.video_pes_expected_len = parse_pes_total_len(&self.video_pes);
            } else if !self.video_pes.is_empty() {
                self.video_pes.extend_from_slice(payload);
            }
            if self.video_pes_expected_len > 0
                && self.video_pes.len() >= self.video_pes_expected_len
            {
                self.flush_video_pes(sink);
            }
        } else if pid == self.audio_pid {
            if pusi {
                if !self.audio_pes.is_empty() {
                    self.flush_audio_pes(sink);
                }
                self.audio_pes.extend_from_slice(payload);
                self.audio_pes_expected_len = parse_pes_total_len(&self.audio_pes);
            } else if !self.audio_pes.is_empty() {
                self.audio_pes.extend_from_slice(payload);
            }
            if self.audio_pes_expected_len > 0
                && self.audio_pes.len() >= self.audio_pes_expected_len
            {
                self.flush_audio_pes(sink);
            }
        }
    }

    fn parse_pat(&mut self, payload: &[u8], pusi: bool) {
        let mut idx = 0;
        if pusi {
            if payload.is_empty() {
                return;
            }
            let pointer = payload[0] as usize;
            idx = 1 + pointer;
        }
        if idx + 8 > payload.len() {
            return;
        }
        let section = &payload[idx..];
        if section[0] != 0x00 {
            return;
        }
        let section_len = (((section[1] & 0x0f) as usize) << 8) | (section[2] as usize);
        if section.len() < 3 + section_len || section_len < 9 {
            return;
        }
        let entries_end = 3 + section_len - 4;
        let mut pos = 8;
        while pos + 4 <= entries_end && pos + 4 <= section.len() {
            let program_num = u16::from_be_bytes([section[pos], section[pos + 1]]);
            let pid = (((section[pos + 2] & 0x1f) as u16) << 8) | (section[pos + 3] as u16);
            if program_num != 0 {
                self.pmt_pid = Some(pid);
                break;
            }
            pos += 4;
        }
    }

    fn parse_pmt(&mut self, payload: &[u8], pusi: bool) {
        let mut idx = 0;
        if pusi {
            if payload.is_empty() {
                return;
            }
            let pointer = payload[0] as usize;
            idx = 1 + pointer;
        }
        if idx + 12 > payload.len() {
            return;
        }
        let section = &payload[idx..];
        if section[0] != 0x02 {
            return;
        }
        let section_len = (((section[1] & 0x0f) as usize) << 8) | (section[2] as usize);
        if section.len() < 3 + section_len || section_len < 13 {
            return;
        }
        let prog_info_len = (((section[10] & 0x0f) as usize) << 8) | (section[11] as usize);
        let mut pos = 12 + prog_info_len;
        let entries_end = 3 + section_len - 4;

        while pos + 5 <= entries_end && pos + 5 <= section.len() {
            let stream_type = section[pos];
            let elem_pid = (((section[pos + 1] & 0x1f) as u16) << 8) | (section[pos + 2] as u16);
            let es_info_len =
                (((section[pos + 3] & 0x0f) as usize) << 8) | (section[pos + 4] as usize);

            match stream_type {
                STREAM_TYPE_H264 | STREAM_TYPE_H265 => {
                    self.video_pid = elem_pid;
                    self.video_stream_type = stream_type;
                }
                STREAM_TYPE_WFD_LPCM
                | STREAM_TYPE_ADTS_AAC
                | STREAM_TYPE_LATM_AAC
                | STREAM_TYPE_MPEG1_AUDIO
                | STREAM_TYPE_MPEG2_AUDIO => {
                    self.audio_pid = elem_pid;
                    self.audio_stream_type = stream_type;
                }
                _ => {}
            }
            pos += 5 + es_info_len;
        }
    }

    fn flush_video_pes(&mut self, sink: &dyn Sink) {
        if self.video_pes.len() < 9 {
            self.video_pes.clear();
            self.video_pes_expected_len = 0;
            return;
        }
        let (_pts_us, payload_offset) = match parse_pes_header(&self.video_pes) {
            Some(v) => v,
            None => {
                self.video_pes.clear();
                self.video_pes_expected_len = 0;
                return;
            }
        };
        if payload_offset >= self.video_pes.len() {
            self.video_pes.clear();
            self.video_pes_expected_len = 0;
            return;
        }

        let es = &self.video_pes[payload_offset..];
        let is_hevc = self.video_stream_type == STREAM_TYPE_H265;
        let nals = split_annexb_nals(es);
        if nals.is_empty() {
            self.video_pes.clear();
            self.video_pes_expected_len = 0;
            return;
        }

        let mut is_keyframe = false;
        let mut sps_changed = false;
        let mut frame_data = Vec::with_capacity(es.len());

        for nal in nals {
            if nal.is_empty() {
                continue;
            }
            if !is_hevc {
                let nal_type = nal[0] & 0x1f;
                match nal_type {
                    7 => {
                        if let Some((w, h)) = parse_h264_sps_dimensions(nal) {
                            if w >= 64 && h >= 64 {
                                self.video_width = w;
                                self.video_height = h;
                            }
                        }
                        if self.configured_sps != nal {
                            self.configured_sps = nal.to_vec();
                            sps_changed = true;
                        }
                    }
                    8 => {
                        if self.configured_pps != nal {
                            self.configured_pps = nal.to_vec();
                            sps_changed = true;
                        }
                    }
                    5 => {
                        is_keyframe = true;
                        frame_data.extend_from_slice(&[0, 0, 0, 1]);
                        frame_data.extend_from_slice(nal);
                    }
                    1 => {
                        frame_data.extend_from_slice(&[0, 0, 0, 1]);
                        frame_data.extend_from_slice(nal);
                    }
                    _ => {}
                }
            } else {
                let nal_type = (nal[0] >> 1) & 0x3f;
                match nal_type {
                    32 => {
                        if self.configured_vps != nal {
                            self.configured_vps = nal.to_vec();
                            sps_changed = true;
                        }
                    }
                    33 => {
                        if let Some((w, h)) = parse_h265_sps_dimensions(nal) {
                            if w >= 64 && h >= 64 {
                                self.video_width = w;
                                self.video_height = h;
                            }
                        }
                        if self.configured_sps != nal {
                            self.configured_sps = nal.to_vec();
                            sps_changed = true;
                        }
                    }
                    34 => {
                        if self.configured_pps != nal {
                            self.configured_pps = nal.to_vec();
                            sps_changed = true;
                        }
                    }
                    16..=21 => {
                        is_keyframe = true;
                        frame_data.extend_from_slice(&[0, 0, 0, 1]);
                        frame_data.extend_from_slice(nal);
                    }
                    0..=9 => {
                        frame_data.extend_from_slice(&[0, 0, 0, 1]);
                        frame_data.extend_from_slice(nal);
                    }
                    _ => {}
                }
            }
        }

        if (!self.video_configured || sps_changed)
            && !self.configured_sps.is_empty()
            && !self.configured_pps.is_empty()
        {
            let mut csd = Vec::new();
            if is_hevc && !self.configured_vps.is_empty() {
                csd.extend_from_slice(&[0, 0, 0, 1]);
                csd.extend_from_slice(&self.configured_vps);
            }
            csd.extend_from_slice(&[0, 0, 0, 1]);
            csd.extend_from_slice(&self.configured_sps);
            csd.extend_from_slice(&[0, 0, 0, 1]);
            csd.extend_from_slice(&self.configured_pps);

            let codec = if is_hevc { Codec::H265 } else { Codec::H264 };
            log::info!(
                "miracast: configuring {:?} decoder {}x{} (csd={} bytes)",
                codec,
                self.video_width,
                self.video_height,
                csd.len()
            );
            sink.video_format(codec, &csd, self.video_width, self.video_height);
            self.video_configured = true;
        }

        if !self.video_configured {
            self.frames_since_sps += 1;
            if self.frames_since_sps % 15 == 1 {
                self.need_idr_request = true;
            }
        } else if !frame_data.is_empty() {
            let now = monotonic_ns();
            sink.video_frame(VideoFrame {
                data: frame_data,
                key: is_keyframe,
                pts_local: 0,
                arrival: now,
            });
        }

        self.video_pes.clear();
        self.video_pes_expected_len = 0;
    }

    fn flush_audio_pes(&mut self, sink: &dyn Sink) {
        if self.audio_pes.len() < 9 {
            self.audio_pes.clear();
            self.audio_pes_expected_len = 0;
            return;
        }
        let (_pts_us, payload_offset) = match parse_pes_header(&self.audio_pes) {
            Some(v) => v,
            None => {
                self.audio_pes.clear();
                self.audio_pes_expected_len = 0;
                return;
            }
        };
        if payload_offset >= self.audio_pes.len() {
            self.audio_pes.clear();
            self.audio_pes_expected_len = 0;
            return;
        }
        let es = &self.audio_pes[payload_offset..];
        let now = monotonic_ns();

        match self.audio_stream_type {
            STREAM_TYPE_WFD_LPCM => {
                // WFD LPCM private stream 1 has a 4-byte subheader followed by 16-bit big-endian stereo PCM
                if es.len() > 4 {
                    let freq_code = (es[2] >> 3) & 0x07;
                    let sample_rate = if freq_code == 1 { 44_100 } else { 48_000 };
                    if !self.audio_configured {
                        let fmt = AudioFormat {
                            codec: AudioCodec::Pcm,
                            spf: 480,
                            sample_rate,
                            channels: 2,
                            using_screen: true,
                            is_media: false,
                        };
                        sink.audio_start(&fmt);
                        self.audio_configured = true;
                    }
                    let pcm_be = &es[4..];
                    for chunk in pcm_be.chunks(1920) {
                        // Convert 16-bit big-endian to 16-bit little-endian for AudioPlayer
                        let mut le_bytes = Vec::with_capacity(chunk.len());
                        for pair in chunk.chunks_exact(2) {
                            le_bytes.push(pair[1]);
                            le_bytes.push(pair[0]);
                        }
                        if !le_bytes.is_empty() {
                            self.audio_seq = self.audio_seq.wrapping_add(1);
                            self.audio_rtp_ts =
                                self.audio_rtp_ts.wrapping_add((le_bytes.len() / 4) as u32);
                            sink.audio_packet(AudioPacket {
                                data: le_bytes,
                                rtp: self.audio_rtp_ts,
                                seq: self.audio_seq,
                                pts_local: now,
                                arrival: now,
                            });
                        }
                    }
                }
            }
            STREAM_TYPE_ADTS_AAC => {
                let mut pos = 0;
                while pos + 7 <= es.len() {
                    if es[pos] != 0xff || (es[pos + 1] & 0xf0) != 0xf0 {
                        pos += 1;
                        continue;
                    }
                    let protection_absent = (es[pos + 1] & 0x01) != 0;
                    let header_len = if protection_absent { 7 } else { 9 };
                    let sf_index = (es[pos + 2] >> 2) & 0x0f;
                    let chan_cfg = ((es[pos + 2] & 0x01) << 2) | ((es[pos + 3] >> 6) & 0x03);
                    let frame_len = ((((es[pos + 3] & 0x03) as usize) << 11)
                        | ((es[pos + 4] as usize) << 3)
                        | (((es[pos + 5] >> 5) & 0x07) as usize))
                        .max(header_len);

                    if pos + frame_len > es.len() {
                        break;
                    }
                    if !self.audio_configured {
                        let sample_rate = adts_sample_rate(sf_index);
                        let fmt = AudioFormat {
                            codec: AudioCodec::AacLc,
                            spf: 1024,
                            sample_rate,
                            channels: (chan_cfg as u32).max(2),
                            using_screen: true,
                            is_media: false,
                        };
                        sink.audio_start(&fmt);
                        self.audio_configured = true;
                    }
                    let raw_aac = &es[pos + header_len..pos + frame_len];
                    if !raw_aac.is_empty() {
                        self.audio_seq = self.audio_seq.wrapping_add(1);
                        self.audio_rtp_ts = self.audio_rtp_ts.wrapping_add(1024);
                        sink.audio_packet(AudioPacket {
                            data: raw_aac.to_vec(),
                            rtp: self.audio_rtp_ts,
                            seq: self.audio_seq,
                            pts_local: now,
                            arrival: now,
                        });
                    }
                    pos += frame_len;
                }
            }
            _ => {}
        }

        self.audio_pes.clear();
        self.audio_pes_expected_len = 0;
    }
}

fn adts_sample_rate(sf_index: u8) -> u32 {
    match sf_index {
        0 => 96_000,
        1 => 88_200,
        2 => 64_000,
        3 => 48_000,
        4 => 44_100,
        5 => 32_000,
        6 => 24_000,
        7 => 22_050,
        8 => 16_000,
        9 => 12_000,
        10 => 11_025,
        11 => 8_000,
        _ => 48_000,
    }
}

fn parse_pes_total_len(pes: &[u8]) -> usize {
    if pes.len() < 6 {
        return 0;
    }
    if pes[0] != 0x00 || pes[1] != 0x00 || pes[2] != 0x01 {
        return 0;
    }
    let pkt_len = u16::from_be_bytes([pes[4], pes[5]]) as usize;
    if pkt_len == 0 {
        0
    } else {
        6 + pkt_len
    }
}

/// Returns `(pts_us, payload_offset)`.
fn parse_pes_header(pes: &[u8]) -> Option<(u64, usize)> {
    if pes.len() < 9 || pes[0] != 0x00 || pes[1] != 0x00 || pes[2] != 0x01 {
        return None;
    }
    let flags2 = pes[7];
    let header_data_len = pes[8] as usize;
    let payload_offset = 9 + header_data_len;
    if payload_offset > pes.len() {
        return None;
    }

    let mut pts_us = 0u64;
    if (flags2 & 0x80) != 0 && header_data_len >= 5 && pes.len() >= 14 {
        let p0 = pes[9] as u64;
        let p1 = pes[10] as u64;
        let p2 = pes[11] as u64;
        let p3 = pes[12] as u64;
        let p4 = pes[13] as u64;
        let pts_90k = (((p0 >> 1) & 0x07) << 30)
            | (p1 << 22)
            | (((p2 >> 1) & 0x7f) << 15)
            | (p3 << 7)
            | ((p4 >> 1) & 0x7f);
        pts_us = (pts_90k * 1_000_000) / 90_000;
    }
    Some((pts_us, payload_offset))
}

fn pes_has_vcl_nal(pes: &[u8], stream_type: u8) -> bool {
    let (_, offset) = match parse_pes_header(pes) {
        Some(v) => v,
        None => return false,
    };
    let is_hevc = stream_type == STREAM_TYPE_H265;
    for nal in split_annexb_nals(&pes[offset..]) {
        if nal.is_empty() {
            continue;
        }
        if !is_hevc {
            let t = nal[0] & 0x1f;
            if t == 1 || t == 5 {
                return true;
            }
        } else {
            let t = (nal[0] >> 1) & 0x3f;
            if t <= 21 {
                return true;
            }
        }
    }
    false
}

/// Split an Annex-B byte stream into individual NAL unit payloads (without start codes).
pub fn split_annexb_nals(data: &[u8]) -> Vec<&[u8]> {
    let mut nals = Vec::new();
    let mut i = 0;
    let len = data.len();
    let mut nal_start: Option<usize> = None;

    while i + 3 <= len {
        let sc_len = if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
            3
        } else if i + 4 <= len
            && data[i] == 0
            && data[i + 1] == 0
            && data[i + 2] == 0
            && data[i + 3] == 1
        {
            4
        } else {
            0
        };

        if sc_len > 0 {
            if let Some(start) = nal_start {
                if start < i {
                    nals.push(&data[start..i]);
                }
            }
            i += sc_len;
            nal_start = Some(i);
        } else {
            i += 1;
        }
    }
    if let Some(start) = nal_start {
        if start < len {
            nals.push(&data[start..len]);
        }
    }
    nals
}

struct BitReader<'a> {
    data: &'a [u8],
    bit_pos: usize,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, bit_pos: 0 }
    }

    fn read_bit(&mut self) -> Option<u32> {
        let byte_idx = self.bit_pos / 8;
        if byte_idx >= self.data.len() {
            return None;
        }
        let bit_idx = 7 - (self.bit_pos % 8);
        self.bit_pos += 1;
        Some(((self.data[byte_idx] >> bit_idx) & 1) as u32)
    }

    fn read_bits(&mut self, n: usize) -> Option<u32> {
        let mut val = 0;
        for _ in 0..n {
            val = (val << 1) | self.read_bit()?;
        }
        Some(val)
    }

    fn read_ue(&mut self) -> Option<u32> {
        let mut zeros = 0;
        while self.read_bit()? == 0 {
            zeros += 1;
            if zeros > 31 {
                return None;
            }
        }
        if zeros == 0 {
            return Some(0);
        }
        let info = self.read_bits(zeros)?;
        Some(((1 << zeros) - 1) + info)
    }

    fn read_se(&mut self) -> Option<i32> {
        let ue = self.read_ue()?;
        let sign = if (ue & 1) != 0 { 1 } else { -1 };
        Some(((ue + 1) / 2) as i32 * sign)
    }
}

/// Strip emulation prevention bytes (`0x00 0x00 0x03` -> `0x00 0x00`) and parse H.264 SPS `(width, height)`.
pub fn parse_h264_sps_dimensions(sps_nal: &[u8]) -> Option<(u32, u32)> {
    if sps_nal.len() < 4 || (sps_nal[0] & 0x1f) != 7 {
        return None;
    }
    let mut rbsp = Vec::with_capacity(sps_nal.len());
    let mut i = 1;
    while i < sps_nal.len() {
        if i + 2 < sps_nal.len()
            && sps_nal[i] == 0x00
            && sps_nal[i + 1] == 0x00
            && sps_nal[i + 2] == 0x03
        {
            rbsp.push(0x00);
            rbsp.push(0x00);
            i += 3;
        } else {
            rbsp.push(sps_nal[i]);
            i += 1;
        }
    }

    let mut br = BitReader::new(&rbsp);
    let profile_idc = br.read_bits(8)?;
    let _constraint_flags = br.read_bits(8)?;
    let _level_idc = br.read_bits(8)?;
    let _sps_id = br.read_ue()?;

    let mut chroma_format_idc = 1;
    if matches!(
        profile_idc,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
    ) {
        chroma_format_idc = br.read_ue()?;
        if chroma_format_idc == 3 {
            let _separate_colour_plane_flag = br.read_bit()?;
        }
        let _bit_depth_luma = br.read_ue()?;
        let _bit_depth_chroma = br.read_ue()?;
        let _qpprime_y_zero = br.read_bit()?;
        let seq_scaling_matrix_present = br.read_bit()?;
        if seq_scaling_matrix_present != 0 {
            let count = if chroma_format_idc != 3 { 8 } else { 12 };
            for idx in 0..count {
                let present = br.read_bit()?;
                if present != 0 {
                    let size = if idx < 6 { 16 } else { 64 };
                    let mut last_scale = 8i32;
                    let mut next_scale = 8i32;
                    for _ in 0..size {
                        if next_scale != 0 {
                            let delta = br.read_se()?;
                            next_scale = (last_scale + delta + 256) % 256;
                        }
                        last_scale = if next_scale == 0 {
                            last_scale
                        } else {
                            next_scale
                        };
                    }
                }
            }
        }
    }

    let _log2_max_frame_num = br.read_ue()?;
    let pic_order_cnt_type = br.read_ue()?;
    if pic_order_cnt_type == 0 {
        let _log2_max_poc_lsb = br.read_ue()?;
    } else if pic_order_cnt_type == 1 {
        let _delta_pic_order_always_zero = br.read_bit()?;
        let _offset_for_non_ref_pic = br.read_se()?;
        let _offset_for_top_to_bottom = br.read_se()?;
        let num_ref_frames_in_poc = br.read_ue()?;
        for _ in 0..num_ref_frames_in_poc {
            let _ = br.read_se()?;
        }
    }

    let _max_num_ref_frames = br.read_ue()?;
    let _gaps_in_frame_num = br.read_bit()?;
    let pic_width_in_mbs_minus1 = br.read_ue()?;
    let pic_height_in_map_units_minus1 = br.read_ue()?;
    let frame_mbs_only_flag = br.read_bit()?;
    if frame_mbs_only_flag == 0 {
        let _mb_adaptive_frame_field = br.read_bit()?;
    }
    let _direct_8x8_inference = br.read_bit()?;
    let frame_cropping_flag = br.read_bit()?;

    let (mut crop_left, mut crop_right, mut crop_top, mut crop_bottom) = (0, 0, 0, 0);
    if frame_cropping_flag != 0 {
        crop_left = br.read_ue()?;
        crop_right = br.read_ue()?;
        crop_top = br.read_ue()?;
        crop_bottom = br.read_ue()?;
    }

    let width = (pic_width_in_mbs_minus1 + 1) * 16;
    let mut height = (2 - frame_mbs_only_flag) * (pic_height_in_map_units_minus1 + 1) * 16;

    let crop_unit_x = if chroma_format_idc == 0 || chroma_format_idc == 3 {
        1
    } else {
        2
    };
    let crop_unit_y = if chroma_format_idc == 1 {
        2 * (2 - frame_mbs_only_flag)
    } else {
        2 - frame_mbs_only_flag
    };

    let final_w = width.saturating_sub((crop_left + crop_right) * crop_unit_x);
    height = height.saturating_sub((crop_top + crop_bottom) * crop_unit_y);

    Some((final_w, height))
}

/// Strip emulation prevention bytes and parse H.265 / HEVC SPS `(width, height)`.
pub fn parse_h265_sps_dimensions(sps_nal: &[u8]) -> Option<(u32, u32)> {
    if sps_nal.len() < 16 || ((sps_nal[0] >> 1) & 0x3f) != 33 {
        return None;
    }
    let mut rbsp = Vec::with_capacity(sps_nal.len());
    let mut i = 2;
    while i < sps_nal.len() {
        if i + 2 < sps_nal.len()
            && sps_nal[i] == 0x00
            && sps_nal[i + 1] == 0x00
            && sps_nal[i + 2] == 0x03
        {
            rbsp.push(0x00);
            rbsp.push(0x00);
            i += 3;
        } else {
            rbsp.push(sps_nal[i]);
            i += 1;
        }
    }

    let mut br = BitReader::new(&rbsp);
    let _vps_id = br.read_bits(4)?;
    let max_sub_layers_minus1 = br.read_bits(3)? as usize;
    if max_sub_layers_minus1 > 6 {
        return None;
    }
    let _temporal_id_nesting = br.read_bit()?;

    // general_profile_tier_level (12 bytes = 96 bits)
    for _ in 0..12 {
        br.read_bits(8)?;
    }

    let mut sub_layer_profile_present = [0u32; 7];
    let mut sub_layer_level_present = [0u32; 7];
    for idx in 0..max_sub_layers_minus1 {
        sub_layer_profile_present[idx] = br.read_bit()?;
        sub_layer_level_present[idx] = br.read_bit()?;
    }
    if max_sub_layers_minus1 > 0 {
        for _ in max_sub_layers_minus1..8 {
            br.read_bits(2)?;
        }
    }
    for idx in 0..max_sub_layers_minus1 {
        if sub_layer_profile_present[idx] != 0 {
            for _ in 0..11 {
                br.read_bits(8)?;
            }
        }
        if sub_layer_level_present[idx] != 0 {
            br.read_bits(8)?;
        }
    }

    let _sps_id = br.read_ue()?;
    let chroma_format_idc = br.read_ue()?;
    if chroma_format_idc == 3 {
        let _separate_colour_plane = br.read_bit()?;
    }
    let pic_width = br.read_ue()?;
    let pic_height = br.read_ue()?;
    let conf_win_flag = br.read_bit()?;

    let mut final_w = pic_width;
    let mut final_h = pic_height;
    if conf_win_flag != 0 {
        let left = br.read_ue()?;
        let right = br.read_ue()?;
        let top = br.read_ue()?;
        let bottom = br.read_ue()?;
        let sub_w = if chroma_format_idc == 1 || chroma_format_idc == 2 {
            2
        } else {
            1
        };
        let sub_h = if chroma_format_idc == 1 { 2 } else { 1 };
        final_w = final_w.saturating_sub((left + right) * sub_w);
        final_h = final_h.saturating_sub((top + bottom) * sub_h);
    }

    Some((final_w, final_h))
}

/// Run the UDP RTP/MPEG-TS receiver loop on `socket` until `stop` is set.
pub fn run_rtp_receiver<F>(
    socket: std::net::UdpSocket,
    initial_width: u32,
    initial_height: u32,
    stop: Arc<AtomicBool>,
    sink: Arc<dyn Sink>,
    mut on_idr_needed: F,
) where
    F: FnMut(),
{
    let _ = socket.set_read_timeout(Some(Duration::from_millis(250)));
    let mut demuxer = TsDemuxer::new(initial_width, initial_height);
    let mut buf = vec![0u8; 65536];
    let mut packet_count = 0u64;
    let mut last_log = Instant::now();

    while !stop.load(Ordering::Relaxed) {
        match socket.recv(&mut buf) {
            Ok(n) if n > 0 => {
                packet_count += 1;
                demuxer.push_udp_packet(&buf[..n], sink.as_ref());
                if demuxer.take_idr_request() {
                    on_idr_needed();
                }
                if last_log.elapsed() >= Duration::from_secs(5) {
                    log::info!(
                        "miracast: RTP active ({} UDP packets received, video={}x{})",
                        packet_count,
                        demuxer.video_width,
                        demuxer.video_height
                    );
                    last_log = Instant::now();
                }
            }
            Ok(_) => {}
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => {
                if !stop.load(Ordering::Relaxed) {
                    log::warn!("miracast: RTP UDP recv error: {}", e);
                }
                break;
            }
        }
    }
    log::info!(
        "miracast: RTP receiver loop stopped after {} packets",
        packet_count
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_h264_sps_1080p() {
        let sps_1080p: [u8; 25] = [
            0x67, 0x64, 0x00, 0x28, 0xac, 0xd9, 0x40, 0x78, 0x02, 0x27, 0xe5, 0xc0, 0x44, 0x00,
            0x00, 0x03, 0x00, 0x04, 0x00, 0x00, 0x03, 0x00, 0xf0, 0x3c, 0x60,
        ];
        let dims = parse_h264_sps_dimensions(&sps_1080p);
        assert_eq!(dims, Some((1920, 1080)));
    }

    #[test]
    fn test_split_annexb_nals() {
        let stream = [
            0x00, 0x00, 0x00, 0x01, 0x67, 0x42, 0x00, 0x1f, 0x00, 0x00, 0x01, 0x68, 0xce, 0x3c,
            0x80, 0x00, 0x00, 0x00, 0x01, 0x65, 0x88, 0x84,
        ];
        let nals = split_annexb_nals(&stream);
        assert_eq!(nals.len(), 3);
        assert_eq!(nals[0], &[0x67, 0x42, 0x00, 0x1f]);
        assert_eq!(nals[1], &[0x68, 0xce, 0x3c, 0x80]);
        assert_eq!(nals[2], &[0x65, 0x88, 0x84]);
    }
}
