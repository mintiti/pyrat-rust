//! Async length-prefixed framing for FlatBuffers messages.
//!
//! Wire format: `[u32 BE payload length][payload bytes]`.
//!
//! [`FrameReader`] and [`FrameWriter`] wrap any `AsyncRead` / `AsyncWrite`
//! stream to send and receive discrete frames over a byte-oriented transport.

use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufWriter};

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
    ///
    /// **Reader:** after this error the stream is desynchronized — the oversized
    /// payload bytes remain unconsumed. The reader should be dropped.
    ///
    /// **Writer:** the payload was not written.
    #[error("payload too large: {size} bytes (max {max})")]
    PayloadTooLarge { size: u64, max: u32 },

    /// Underlying I/O error.
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Reads length-prefixed frames from an async byte stream.
///
/// Wire format: `[u32 BE payload length][payload bytes]`
///
/// **Cancel-safe:** partial read progress is tracked in the struct, so dropping
/// a `read_frame` future mid-read and calling it again resumes correctly. This
/// is essential when used inside `tokio::select!`.
///
/// Reuses an internal buffer across reads (high-water mark — never shrinks).
/// The returned `&[u8]` borrows from this buffer, so the caller must finish
/// parsing before calling `read_frame` again.
pub struct FrameReader<R> {
    reader: R,
    buf: Vec<u8>,
    max_payload: u32,
    // Cancel-safe state: persists across dropped read_frame() futures.
    header: [u8; 4],
    header_pos: u8,
    payload_pos: usize,
}

impl<R: AsyncRead + Unpin> FrameReader<R> {
    #[must_use]
    pub fn new(reader: R, max_payload: u32) -> Self {
        Self {
            reader,
            buf: Vec::with_capacity(4096),
            max_payload,
            header: [0; 4],
            header_pos: 0,
            payload_pos: 0,
        }
    }

    /// Create a reader with [`DEFAULT_MAX_PAYLOAD`].
    #[must_use]
    pub fn with_default_max(reader: R) -> Self {
        Self::new(reader, DEFAULT_MAX_PAYLOAD)
    }

    /// Read the next frame, returning the payload bytes.
    ///
    /// Returns `FrameError::Disconnected` on a clean EOF at the frame boundary.
    ///
    /// After a [`FrameError::PayloadTooLarge`] error the stream is
    /// desynchronized (the oversized payload remains unconsumed). The reader
    /// should be dropped — subsequent reads will return garbage.
    pub async fn read_frame(&mut self) -> Result<&[u8], FrameError> {
        // --- Read the 4-byte length header ---
        // Progress is tracked in self.header / self.header_pos so that
        // dropping this future mid-read and calling it again resumes correctly.
        while self.header_pos < 4 {
            let pos = self.header_pos as usize;
            let n = self.reader.read(&mut self.header[pos..4]).await?;
            if n == 0 {
                return if self.header_pos == 0 {
                    Err(FrameError::Disconnected)
                } else {
                    self.header_pos = 0;
                    Err(FrameError::UnexpectedEof)
                };
            }
            self.header_pos += n as u8;
        }

        let payload_len = u32::from_be_bytes(self.header);

        // --- Guard against OOM ---
        if payload_len > self.max_payload {
            self.header_pos = 0;
            self.payload_pos = 0;
            return Err(FrameError::PayloadTooLarge {
                size: u64::from(payload_len),
                max: self.max_payload,
            });
        }

        let target = payload_len as usize;

        // Grow buffer if needed (never shrinks).
        if self.buf.len() < target {
            self.buf.resize(target, 0);
        }

        // --- Read payload ---
        while self.payload_pos < target {
            let n = self
                .reader
                .read(&mut self.buf[self.payload_pos..target])
                .await?;
            if n == 0 {
                self.header_pos = 0;
                self.payload_pos = 0;
                return Err(FrameError::UnexpectedEof);
            }
            self.payload_pos += n;
        }

        // --- Complete: reset and return ---
        self.header_pos = 0;
        self.payload_pos = 0;
        Ok(&self.buf[..target])
    }

    /// Consume the reader, returning the underlying stream.
    #[must_use]
    pub fn into_inner(self) -> R {
        self.reader
    }
}

/// Writes length-prefixed frames to an async byte stream.
///
/// Wire format: `[u32 BE payload length][payload bytes]`
///
/// Wraps the inner writer in a [`BufWriter`] so that the header and payload
/// are coalesced into a single syscall, flushed at the end of each frame.
pub struct FrameWriter<W> {
    writer: BufWriter<W>,
    max_payload: u32,
}

impl<W: AsyncWrite + Unpin> FrameWriter<W> {
    #[must_use]
    pub fn new(writer: W, max_payload: u32) -> Self {
        Self {
            writer: BufWriter::new(writer),
            max_payload,
        }
    }

    /// Create a writer with [`DEFAULT_MAX_PAYLOAD`].
    #[must_use]
    pub fn with_default_max(writer: W) -> Self {
        Self::new(writer, DEFAULT_MAX_PAYLOAD)
    }

    /// Write a single frame (length prefix + payload) and flush.
    pub async fn write_frame(&mut self, payload: &[u8]) -> Result<(), FrameError> {
        let len: u32 = payload
            .len()
            .try_into()
            .map_err(|_| FrameError::PayloadTooLarge {
                size: payload.len() as u64,
                max: self.max_payload,
            })?;

        if len > self.max_payload {
            return Err(FrameError::PayloadTooLarge {
                size: u64::from(len),
                max: self.max_payload,
            });
        }

        self.writer.write_all(&len.to_be_bytes()).await?;
        self.writer.write_all(payload).await?;
        self.writer.flush().await?;
        Ok(())
    }

    /// Consume the writer, returning the underlying stream.
    #[must_use]
    pub fn into_inner(self) -> W {
        self.writer.into_inner()
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{duplex, AsyncWriteExt};

    use super::*;

    #[tokio::test]
    async fn test_round_trip_single_frame() {
        let (client, server) = duplex(1024);
        let mut writer = FrameWriter::with_default_max(client);
        let mut reader = FrameReader::with_default_max(server);

        let payload = b"hello flatbuffers";
        writer.write_frame(payload).await.unwrap();
        drop(writer);

        let got = reader.read_frame().await.unwrap();
        assert_eq!(got, payload);
    }

    #[tokio::test]
    async fn test_round_trip_multiple_frames() {
        let (client, server) = duplex(4096);
        let mut writer = FrameWriter::with_default_max(client);
        let mut reader = FrameReader::with_default_max(server);

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
    async fn test_large_frame_near_max() {
        let max = 64 * 1024; // 64 KB for this test
        let (client, server) = duplex(max + 256);
        let mut writer = FrameWriter::new(client, max as u32);
        let mut reader = FrameReader::new(server, max as u32);

        let payload = vec![0xABu8; max];
        writer.write_frame(&payload).await.unwrap();
        drop(writer);

        let got = reader.read_frame().await.unwrap();
        assert_eq!(got.len(), max);
        assert!(got.iter().all(|&b| b == 0xAB));
    }

    #[tokio::test]
    async fn test_reader_payload_too_large() {
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
    async fn test_writer_payload_too_large() {
        let (client, _server) = duplex(1024);
        let mut writer = FrameWriter::new(client, 50); // max 50 bytes

        let payload = vec![0u8; 100];
        let err = writer.write_frame(&payload).await.unwrap_err();
        assert!(
            matches!(err, FrameError::PayloadTooLarge { size: 100, max: 50 }),
            "expected PayloadTooLarge, got {err:?}"
        );
    }

    #[tokio::test]
    async fn test_clean_disconnect() {
        let (client, server) = duplex(1024);
        drop(client); // immediate close

        let mut reader = FrameReader::with_default_max(server);
        let err = reader.read_frame().await.unwrap_err();
        assert!(
            matches!(err, FrameError::Disconnected),
            "expected Disconnected, got {err:?}"
        );
    }

    #[tokio::test]
    async fn test_mid_header_eof() {
        // Write only 2 of the 4 header bytes.
        let (mut client, server) = duplex(1024);
        client.write_all(&[0u8; 2]).await.unwrap();
        client.shutdown().await.unwrap();

        let mut reader = FrameReader::with_default_max(server);
        let err = reader.read_frame().await.unwrap_err();
        assert!(
            matches!(err, FrameError::UnexpectedEof),
            "expected UnexpectedEof, got {err:?}"
        );
    }

    #[tokio::test]
    async fn test_mid_payload_eof() {
        // Write a valid header claiming 100 bytes, then only send 10.
        let (mut client, server) = duplex(1024);
        let len: u32 = 100;
        client.write_all(&len.to_be_bytes()).await.unwrap();
        client.write_all(&[0u8; 10]).await.unwrap();
        client.shutdown().await.unwrap();

        let mut reader = FrameReader::with_default_max(server);
        let err = reader.read_frame().await.unwrap_err();
        assert!(
            matches!(err, FrameError::UnexpectedEof),
            "expected UnexpectedEof, got {err:?}"
        );
    }

    #[tokio::test]
    async fn test_empty_payload() {
        let (client, server) = duplex(1024);
        let mut writer = FrameWriter::with_default_max(client);
        let mut reader = FrameReader::with_default_max(server);

        writer.write_frame(b"").await.unwrap();
        drop(writer);

        let got = reader.read_frame().await.unwrap();
        assert!(got.is_empty());
    }

    #[tokio::test]
    async fn test_buffer_reuse_across_frames() {
        let (client, server) = duplex(4096);
        let mut writer = FrameWriter::with_default_max(client);
        let mut reader = FrameReader::with_default_max(server);

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

    #[tokio::test]
    async fn test_into_inner() {
        let (client, server) = duplex(1024);
        let mut writer = FrameWriter::with_default_max(client);

        writer.write_frame(b"before into_inner").await.unwrap();
        drop(writer);

        let reader = FrameReader::with_default_max(server);
        let inner = reader.into_inner();

        // The stream is still usable — we can wrap it again and read.
        let mut reader2 = FrameReader::with_default_max(inner);
        let got = reader2.read_frame().await.unwrap();
        assert_eq!(got, b"before into_inner");
    }

    #[tokio::test]
    async fn test_read_frame_after_payload_too_large() {
        // After PayloadTooLarge the stream is desynchronized — demonstrate this.
        let (mut client, server) = duplex(1024);

        // Frame 1: header claims 100 bytes, reader max is 50 → PayloadTooLarge.
        let fake_len: u32 = 100;
        client.write_all(&fake_len.to_be_bytes()).await.unwrap();
        client.write_all(&[0xAA; 100]).await.unwrap();

        // Frame 2: a normal frame that should be readable if the stream were ok.
        let good_len: u32 = 5;
        client.write_all(&good_len.to_be_bytes()).await.unwrap();
        client.write_all(b"hello").await.unwrap();
        client.shutdown().await.unwrap();

        let mut reader = FrameReader::new(server, 50);

        // First read: PayloadTooLarge as expected.
        let err = reader.read_frame().await.unwrap_err();
        assert!(matches!(err, FrameError::PayloadTooLarge { .. }));

        // Second read: the 100 payload bytes are still in the stream, so the
        // reader interprets them as a header → garbage. This demonstrates why
        // the reader should be dropped after PayloadTooLarge.
        let result = reader.read_frame().await;
        // Could be any error or a nonsensical "success" — the point is the
        // stream is broken. We just assert it doesn't return the clean "hello".
        if let Ok(data) = result {
            assert_ne!(data, b"hello", "stream should be desynchronized");
        }
    }

    #[tokio::test]
    async fn test_cancel_safety_header_read_then_resume() {
        // Simulate the select! cancellation scenario: header arrives but payload
        // hasn't yet. The read_frame future is dropped (cancelled). A second
        // call must resume and read the payload correctly.
        let (mut write_end, read_end) = duplex(64);
        let mut reader = FrameReader::with_default_max(read_end);

        // Write only the 4-byte header (payload = 5 bytes, not yet sent).
        let len: u32 = 5;
        write_end.write_all(&len.to_be_bytes()).await.unwrap();

        // Start read_frame, cancel it via select! when it blocks on the payload.
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
        cancel_tx.send(()).unwrap();
        tokio::select! {
            biased;
            // read_frame will read the header, then block waiting for payload.
            // On the next poll, select! sees cancel_rx is ready and picks it.
            _ = reader.read_frame() => panic!("should not complete — payload not sent yet"),
            _ = cancel_rx => { /* cancelled as expected */ }
        }

        // Now send the payload.
        write_end.write_all(b"hello").await.unwrap();

        // read_frame must resume from saved state and return the complete frame.
        let frame = reader.read_frame().await.unwrap();
        assert_eq!(frame, b"hello");

        // Subsequent frames work normally.
        write_end.write_all(&5u32.to_be_bytes()).await.unwrap();
        write_end.write_all(b"world").await.unwrap();
        drop(write_end);

        let frame2 = reader.read_frame().await.unwrap();
        assert_eq!(frame2, b"world");
    }
}
