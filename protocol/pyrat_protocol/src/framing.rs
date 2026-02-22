use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Default maximum payload size: 16 MB.
pub const DEFAULT_MAX_PAYLOAD: u32 = 16 * 1024 * 1024;

/// Errors that can occur during frame reading/writing.
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    /// Clean EOF on header boundary — the peer closed the connection.
    #[error("peer disconnected")]
    Disconnected,

    /// EOF mid-header or mid-payload — the peer crashed or the connection broke.
    #[error("unexpected EOF mid-frame")]
    UnexpectedEof,

    /// The declared payload length exceeds the configured maximum.
    #[error("payload too large: {size} bytes (max {max})")]
    PayloadTooLarge { size: u32, max: u32 },

    /// Underlying I/O error.
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Reads length-prefixed frames from an async byte stream.
///
/// Wire format: `[u32 BE payload length][payload bytes]`
///
/// Reuses an internal buffer across reads (high-water mark — never shrinks).
/// The returned `&[u8]` borrows from this buffer, so the caller must finish
/// parsing before calling `read_frame` again.
pub struct FrameReader<R> {
    reader: R,
    buf: Vec<u8>,
    max_payload: u32,
}

impl<R: AsyncRead + Unpin> FrameReader<R> {
    pub fn new(reader: R, max_payload: u32) -> Self {
        Self {
            reader,
            buf: Vec::with_capacity(4096),
            max_payload,
        }
    }

    /// Read the next frame, returning the payload bytes.
    ///
    /// Returns `FrameError::Disconnected` on a clean EOF at the frame boundary.
    pub async fn read_frame(&mut self) -> Result<&[u8], FrameError> {
        // --- Read the 4-byte length header ---
        let mut header = [0u8; 4];
        match self.reader.read_exact(&mut header).await {
            Ok(_) => {},
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                // Could be clean disconnect (0 bytes read) or partial header.
                // read_exact doesn't tell us how many bytes were read before EOF,
                // but we can distinguish by trying to read a single byte first.
                // However, read_exact already consumed the stream — we can't retry.
                //
                // Heuristic: if we get UnexpectedEof on the very first read_exact
                // for the header, treat it as disconnected. The only way to get a
                // *partial* header EOF in practice is a broken connection, which is
                // rare enough that conflating with Disconnected is acceptable.
                //
                // For precise detection, we do a manual 1-byte probe below instead.
                return Err(FrameError::Disconnected);
            },
            Err(e) => return Err(FrameError::Io(e)),
        }

        let payload_len = u32::from_be_bytes(header);

        // --- Guard against OOM ---
        if payload_len > self.max_payload {
            return Err(FrameError::PayloadTooLarge {
                size: payload_len,
                max: self.max_payload,
            });
        }

        let len = payload_len as usize;

        // Grow buffer if needed (never shrinks).
        if self.buf.len() < len {
            self.buf.resize(len, 0);
        }

        // --- Read payload ---
        if len > 0 {
            self.reader
                .read_exact(&mut self.buf[..len])
                .await
                .map_err(|e| {
                    if e.kind() == io::ErrorKind::UnexpectedEof {
                        FrameError::UnexpectedEof
                    } else {
                        FrameError::Io(e)
                    }
                })?;
        }

        Ok(&self.buf[..len])
    }

    /// Consume the reader, returning the underlying stream.
    pub fn into_inner(self) -> R {
        self.reader
    }
}

/// Writes length-prefixed frames to an async byte stream.
///
/// Wire format: `[u32 BE payload length][payload bytes]`
pub struct FrameWriter<W> {
    writer: W,
}

impl<W: AsyncWrite + Unpin> FrameWriter<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    /// Write a single frame (length prefix + payload) and flush.
    pub async fn write_frame(&mut self, payload: &[u8]) -> Result<(), FrameError> {
        let len: u32 = payload
            .len()
            .try_into()
            .expect("payload exceeds u32::MAX bytes");

        self.writer.write_all(&len.to_be_bytes()).await?;
        self.writer.write_all(payload).await?;
        self.writer.flush().await?;
        Ok(())
    }

    /// Consume the writer, returning the underlying stream.
    pub fn into_inner(self) -> W {
        self.writer
    }
}

// ──────────────────────────────────────────────
// Precise disconnect detection
// ──────────────────────────────────────────────
//
// The simple read_exact approach above conflates "0 bytes available" (clean
// disconnect) with "1-3 bytes available then EOF" (broken connection) because
// tokio's read_exact doesn't expose how many bytes it managed to read.
//
// For the host we want the distinction, so FrameReader uses a two-step read:
//   1. read(&mut header[0..1]) — if returns 0, that's Disconnected.
//   2. read_exact(&mut header[1..4]) — if EOF here, that's UnexpectedEof.
//
// We keep the implementation above clean and add precise detection with a
// re-implementation that replaces the header read logic.

impl<R: AsyncRead + Unpin> FrameReader<R> {
    /// Like `read_frame`, but distinguishes a clean disconnect (EOF before any
    /// header bytes) from a mid-header EOF.
    pub async fn read_frame_precise(&mut self) -> Result<&[u8], FrameError> {
        let mut header = [0u8; 4];

        // Step 1: probe the first byte.
        let n = self.reader.read(&mut header[..1]).await?;
        if n == 0 {
            return Err(FrameError::Disconnected);
        }

        // Step 2: read remaining 3 header bytes.
        self.reader
            .read_exact(&mut header[1..])
            .await
            .map_err(|e| {
                if e.kind() == io::ErrorKind::UnexpectedEof {
                    FrameError::UnexpectedEof
                } else {
                    FrameError::Io(e)
                }
            })?;

        let payload_len = u32::from_be_bytes(header);

        if payload_len > self.max_payload {
            return Err(FrameError::PayloadTooLarge {
                size: payload_len,
                max: self.max_payload,
            });
        }

        let len = payload_len as usize;

        if self.buf.len() < len {
            self.buf.resize(len, 0);
        }

        if len > 0 {
            self.reader
                .read_exact(&mut self.buf[..len])
                .await
                .map_err(|e| {
                    if e.kind() == io::ErrorKind::UnexpectedEof {
                        FrameError::UnexpectedEof
                    } else {
                        FrameError::Io(e)
                    }
                })?;
        }

        Ok(&self.buf[..len])
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::duplex;

    use super::*;

    #[tokio::test]
    async fn round_trip_single_frame() {
        let (client, server) = duplex(1024);
        let mut writer = FrameWriter::new(client);
        let mut reader = FrameReader::new(server, DEFAULT_MAX_PAYLOAD);

        let payload = b"hello flatbuffers";
        writer.write_frame(payload).await.unwrap();
        drop(writer); // close write half

        let got = reader.read_frame().await.unwrap();
        assert_eq!(got, payload);
    }

    #[tokio::test]
    async fn round_trip_multiple_frames() {
        let (client, server) = duplex(4096);
        let mut writer = FrameWriter::new(client);
        let mut reader = FrameReader::new(server, DEFAULT_MAX_PAYLOAD);

        let payloads: &[&[u8]] = &[b"one", b"two two", b"three three three"];
        for p in payloads {
            writer.write_frame(p).await.unwrap();
        }
        drop(writer);

        for expected in payloads {
            let got = reader.read_frame().await.unwrap();
            assert_eq!(got, *expected);
        }
    }

    #[tokio::test]
    async fn large_frame_near_max() {
        let max = 64 * 1024; // 64 KB for this test
        let (client, server) = duplex(max + 256);
        let mut writer = FrameWriter::new(client);
        let mut reader = FrameReader::new(server, max as u32);

        let payload = vec![0xABu8; max];
        writer.write_frame(&payload).await.unwrap();
        drop(writer);

        let got = reader.read_frame().await.unwrap();
        assert_eq!(got.len(), max);
        assert!(got.iter().all(|&b| b == 0xAB));
    }

    #[tokio::test]
    async fn payload_too_large() {
        // Manually write a header claiming a huge payload.
        let (mut client, server) = duplex(1024);
        let fake_len: u32 = 100;
        client.write_all(&fake_len.to_be_bytes()).await.unwrap();
        client.shutdown().await.unwrap();

        let mut reader = FrameReader::new(server, 50); // max 50 bytes

        let err = reader.read_frame().await.unwrap_err();
        assert!(
            matches!(err, FrameError::PayloadTooLarge { size: 100, max: 50 }),
            "expected PayloadTooLarge, got {err:?}"
        );
    }

    #[tokio::test]
    async fn clean_disconnect() {
        let (client, server) = duplex(1024);
        drop(client); // immediate close

        let mut reader = FrameReader::new(server, DEFAULT_MAX_PAYLOAD);
        let err = reader.read_frame().await.unwrap_err();
        assert!(
            matches!(err, FrameError::Disconnected),
            "expected Disconnected, got {err:?}"
        );
    }

    #[tokio::test]
    async fn mid_payload_eof() {
        // Write a valid header claiming 100 bytes, then only send 10.
        let (mut client, server) = duplex(1024);
        let len: u32 = 100;
        client.write_all(&len.to_be_bytes()).await.unwrap();
        client.write_all(&[0u8; 10]).await.unwrap();
        client.shutdown().await.unwrap();

        let mut reader = FrameReader::new(server, DEFAULT_MAX_PAYLOAD);
        let err = reader.read_frame().await.unwrap_err();
        assert!(
            matches!(err, FrameError::UnexpectedEof),
            "expected UnexpectedEof, got {err:?}"
        );
    }

    #[tokio::test]
    async fn empty_payload() {
        let (client, server) = duplex(1024);
        let mut writer = FrameWriter::new(client);
        let mut reader = FrameReader::new(server, DEFAULT_MAX_PAYLOAD);

        writer.write_frame(b"").await.unwrap();
        drop(writer);

        let got = reader.read_frame().await.unwrap();
        assert!(got.is_empty());
    }

    #[tokio::test]
    async fn precise_clean_disconnect() {
        let (client, server) = duplex(1024);
        drop(client);

        let mut reader = FrameReader::new(server, DEFAULT_MAX_PAYLOAD);
        let err = reader.read_frame_precise().await.unwrap_err();
        assert!(
            matches!(err, FrameError::Disconnected),
            "expected Disconnected, got {err:?}"
        );
    }

    #[tokio::test]
    async fn precise_mid_header_eof() {
        // Write only 2 of the 4 header bytes.
        let (mut client, server) = duplex(1024);
        client.write_all(&[0u8; 2]).await.unwrap();
        client.shutdown().await.unwrap();

        let mut reader = FrameReader::new(server, DEFAULT_MAX_PAYLOAD);
        let err = reader.read_frame_precise().await.unwrap_err();
        assert!(
            matches!(err, FrameError::UnexpectedEof),
            "expected UnexpectedEof, got {err:?}"
        );
    }

    #[tokio::test]
    async fn buffer_reuse_across_frames() {
        let (client, server) = duplex(4096);
        let mut writer = FrameWriter::new(client);
        let mut reader = FrameReader::new(server, DEFAULT_MAX_PAYLOAD);

        // Write a large frame then a small one — buffer should stay at high-water mark.
        let large = vec![1u8; 2000];
        let small = b"tiny";

        writer.write_frame(&large).await.unwrap();
        writer.write_frame(small).await.unwrap();
        drop(writer);

        let got1 = reader.read_frame().await.unwrap();
        assert_eq!(got1.len(), 2000);

        let buf_capacity_after_large = reader.buf.len();

        let got2 = reader.read_frame().await.unwrap();
        assert_eq!(got2, b"tiny");

        // Buffer didn't shrink.
        assert_eq!(reader.buf.len(), buf_capacity_after_large);
    }
}
