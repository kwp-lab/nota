use anyhow::{Context, Result};
use ogg::{PacketWriteEndInfo, PacketWriter};
use opus::{Application, Bitrate, Channels, Encoder};
use rand::Rng;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use super::SAMPLE_RATE;

const OPUS_FRAME_SAMPLES: usize = 960;

pub struct OpusOggWriter {
    path: PathBuf,
    packet_writer: PacketWriter<'static, BufWriter<File>>,
    encoder: Encoder,
    serial: u32,
    granule_position: u64,
    frame_buffer: Vec<f32>,
    pending_packet: Option<Vec<u8>>,
    packet_count: u64,
}

impl OpusOggWriter {
    pub fn create(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .with_context(|| format!("无法创建恢复文件 {}", path.display()))?;
        let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Mono, Application::Voip)?;
        encoder.set_bitrate(Bitrate::Bits(64_000))?;
        encoder.set_vbr(true)?;
        encoder.set_complexity(8)?;
        let pre_skip = encoder.get_lookahead()?.max(0) as u16;
        let serial = rand::rng().random::<u32>();
        let mut packet_writer = PacketWriter::new(BufWriter::with_capacity(64 * 1024, file));
        packet_writer.write_packet(opus_head(pre_skip), serial, PacketWriteEndInfo::EndPage, 0)?;
        packet_writer.write_packet(opus_tags(), serial, PacketWriteEndInfo::EndPage, 0)?;
        packet_writer.inner_mut().flush()?;
        Ok(Self {
            path: path.to_path_buf(),
            packet_writer,
            encoder,
            serial,
            granule_position: pre_skip as u64,
            frame_buffer: Vec::with_capacity(OPUS_FRAME_SAMPLES * 2),
            pending_packet: None,
            packet_count: 0,
        })
    }

    pub fn push_frame(&mut self, frame: &[f32]) -> Result<()> {
        self.frame_buffer.extend_from_slice(frame);
        while self.frame_buffer.len() >= OPUS_FRAME_SAMPLES {
            let input: Vec<f32> = self.frame_buffer.drain(..OPUS_FRAME_SAMPLES).collect();
            let mut encoded = vec![0u8; 4_000];
            let length = self.encoder.encode_float(&input, &mut encoded)?;
            encoded.truncate(length);
            if let Some(previous) = self.pending_packet.replace(encoded) {
                self.granule_position += OPUS_FRAME_SAMPLES as u64;
                self.packet_count += 1;
                let end_info = if self.packet_count.is_multiple_of(50) {
                    PacketWriteEndInfo::EndPage
                } else {
                    PacketWriteEndInfo::NormalPacket
                };
                self.packet_writer.write_packet(
                    previous,
                    self.serial,
                    end_info,
                    self.granule_position,
                )?;
                if self.packet_count.is_multiple_of(50) {
                    self.packet_writer.inner_mut().flush()?;
                }
                if self.packet_count.is_multiple_of(250) {
                    self.packet_writer.inner_mut().get_ref().sync_data()?;
                }
            }
        }
        Ok(())
    }

    pub fn bytes_written(&self) -> u64 {
        self.packet_writer
            .inner()
            .get_ref()
            .metadata()
            .map(|metadata| metadata.len())
            .unwrap_or(0)
    }

    pub fn finish(mut self) -> Result<PathBuf> {
        if self.frame_buffer.is_empty() && self.pending_packet.is_none() {
            self.frame_buffer.resize(OPUS_FRAME_SAMPLES, 0.0);
        }
        if !self.frame_buffer.is_empty() {
            self.frame_buffer.resize(OPUS_FRAME_SAMPLES, 0.0);
            let mut encoded = vec![0u8; 4_000];
            let length = self
                .encoder
                .encode_float(&self.frame_buffer, &mut encoded)?;
            encoded.truncate(length);
            if let Some(previous) = self.pending_packet.replace(encoded) {
                self.granule_position += OPUS_FRAME_SAMPLES as u64;
                self.packet_writer.write_packet(
                    previous,
                    self.serial,
                    PacketWriteEndInfo::NormalPacket,
                    self.granule_position,
                )?;
            }
        }
        if let Some(last) = self.pending_packet.take() {
            self.granule_position += OPUS_FRAME_SAMPLES as u64;
            self.packet_writer.write_packet(
                last,
                self.serial,
                PacketWriteEndInfo::EndStream,
                self.granule_position,
            )?;
        }
        self.packet_writer.inner_mut().flush()?;
        self.packet_writer.inner().get_ref().sync_all()?;
        Ok(self.path)
    }
}

fn opus_head(pre_skip: u16) -> Vec<u8> {
    let mut packet = Vec::with_capacity(19);
    packet.extend_from_slice(b"OpusHead");
    packet.push(1);
    packet.push(1);
    packet.extend_from_slice(&pre_skip.to_le_bytes());
    packet.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    packet.extend_from_slice(&0i16.to_le_bytes());
    packet.push(0);
    packet
}

fn opus_tags() -> Vec<u8> {
    let vendor = b"Nota 0.1";
    let mut packet = Vec::new();
    packet.extend_from_slice(b"OpusTags");
    packet.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    packet.extend_from_slice(vendor);
    packet.extend_from_slice(&1u32.to_le_bytes());
    let comment = b"ENCODER=Nota";
    packet.extend_from_slice(&(comment.len() as u32).to_le_bytes());
    packet.extend_from_slice(comment);
    packet
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn ogg_can_be_truncated_to_last_complete_page_and_recovered() {
        let root = std::env::temp_dir().join(format!("nota-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let partial = root.join("session.partial.ogg");
        let destination = root.join("recovered.ogg");
        let mut writer = OpusOggWriter::create(&partial).unwrap();
        for _ in 0..120 {
            writer.push_frame(&vec![0.05; 480]).unwrap();
        }
        writer.finish().unwrap();
        let valid_size = std::fs::metadata(&partial).unwrap().len();
        OpenOptions::new()
            .append(true)
            .open(&partial)
            .unwrap()
            .write_all(b"OggS-incomplete-tail")
            .unwrap();
        crate::audio::recover_ogg_file(&partial, &destination).unwrap();
        assert!(destination.exists());
        assert!(std::fs::metadata(&destination).unwrap().len() <= valid_size + 27);
        let _ = std::fs::remove_dir_all(root);
    }
}
