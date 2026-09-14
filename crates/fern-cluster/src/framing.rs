//! Retain partial read progress across select cancellation; never retry partial writes.
use crate::wire::invalid;
use crate::{Frame, Hello, MAX_PEER_FRAME_BYTES};
use std::{io, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::Instant;

#[derive(Clone, Copy, Debug)]
pub struct IoLimits {
    pub handshake_timeout: Duration,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
}
impl Default for IoLimits {
    fn default() -> Self {
        Self {
            handshake_timeout: Duration::from_secs(10),
            read_timeout: Duration::from_secs(30),
            write_timeout: Duration::from_secs(2),
        }
    }
}
impl IoLimits {
    pub(crate) fn validate(self) -> io::Result<Self> {
        if [
            self.handshake_timeout,
            self.read_timeout,
            self.write_timeout,
        ]
        .iter()
        .any(|d| d.is_zero() || *d > Duration::from_secs(30))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid peer IO deadlines",
            ));
        }
        Ok(self)
    }
}

pub struct FrameReader<R> {
    io: R,
    limits: IoLimits,
    prefix: [u8; 4],
    prefix_read: usize,
    body: Vec<u8>,
    body_read: usize,
    deadline: Option<Instant>,
    failed: bool,
    ended: bool,
}
impl<R: AsyncRead + Unpin> FrameReader<R> {
    pub fn new(io: R, limits: IoLimits) -> io::Result<Self> {
        Ok(Self {
            io,
            limits: limits.validate()?,
            prefix: [0; 4],
            prefix_read: 0,
            body: Vec::new(),
            body_read: 0,
            deadline: None,
            failed: false,
            ended: false,
        })
    }
    /// Safe to cancel this future: the next call resumes the same bounded frame and deadline.
    pub async fn read(&mut self) -> io::Result<Option<Frame>> {
        self.read_with(Frame::decode).await
    }
    pub(crate) async fn read_hello(&mut self) -> io::Result<Option<Hello>> {
        self.read_with(Hello::decode).await
    }
    /// Allocation observation for admission tests; at most one bounded frame is retained.
    pub fn buffered_capacity(&self) -> usize {
        self.body.capacity()
    }
    async fn read_with<T>(
        &mut self,
        decode: impl FnOnce(&[u8]) -> io::Result<T>,
    ) -> io::Result<Option<T>> {
        if self.failed {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "peer reader is closed after failure",
            ));
        }
        if self.ended {
            return Ok(None);
        }
        let result = self.read_inner(decode).await;
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    async fn read_inner<T>(
        &mut self,
        decode: impl FnOnce(&[u8]) -> io::Result<T>,
    ) -> io::Result<Option<T>> {
        let deadline = *self
            .deadline
            .get_or_insert_with(|| Instant::now() + self.limits.read_timeout);
        while self.prefix_read < 4 {
            check_deadline(deadline)?;
            let read = tokio::time::timeout_at(
                deadline,
                self.io.read(&mut self.prefix[self.prefix_read..]),
            )
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "peer frame read deadline"))??;
            if read == 0 {
                if self.prefix_read == 0 {
                    self.ended = true;
                    return Ok(None);
                }
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "truncated peer length prefix",
                ));
            }
            self.prefix_read += read;
        }
        if self.body.is_empty() {
            let length = u32::from_be_bytes(self.prefix) as usize;
            if length == 0 || length > MAX_PEER_FRAME_BYTES {
                return Err(invalid("invalid peer frame length"));
            }
            if self.body.capacity() < length {
                // One fixed upper bound avoids Vec's geometric capacity overshoot.
                self.body = Vec::new();
                self.body = Vec::with_capacity(MAX_PEER_FRAME_BYTES);
            }
            self.body.resize(length, 0);
        }
        while self.body_read < self.body.len() {
            check_deadline(deadline)?;
            let read =
                tokio::time::timeout_at(deadline, self.io.read(&mut self.body[self.body_read..]))
                    .await
                    .map_err(|_| {
                        io::Error::new(io::ErrorKind::TimedOut, "peer frame read deadline")
                    })??;
            if read == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "truncated peer frame body",
                ));
            }
            self.body_read += read;
        }
        check_deadline(deadline)?;
        let value = decode(&self.body)?;
        self.prefix_read = 0;
        self.body_read = 0;
        self.body.clear();
        self.deadline = None;
        Ok(Some(value))
    }
}

pub struct FrameWriter<W> {
    io: W,
    limits: IoLimits,
    failed: bool,
}
impl<W: AsyncWrite + Unpin> FrameWriter<W> {
    pub fn new(io: W, limits: IoLimits) -> io::Result<Self> {
        Ok(Self {
            io,
            limits: limits.validate()?,
            failed: false,
        })
    }
    /// Cancellation or partial IO failure poisons the writer; close the stream, never replay.
    pub async fn write(&mut self, frame: &Frame) -> io::Result<()> {
        self.write_payload(frame.encode()?).await
    }
    pub(crate) async fn write_hello(&mut self, hello: &Hello) -> io::Result<()> {
        self.write_payload(hello.encode()?).await
    }
    async fn write_payload(&mut self, payload: Vec<u8>) -> io::Result<()> {
        if self.failed {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "peer writer cannot resume a partial frame",
            ));
        }
        let prefix = u32::try_from(payload.len())
            .map_err(|_| invalid("peer frame length overflow"))?
            .to_be_bytes();
        // Remains set if this future is dropped while any IO operation is pending.
        self.failed = true;
        let deadline = Instant::now() + self.limits.write_timeout;
        tokio::time::timeout_at(deadline, async {
            self.io.write_all(&prefix).await?;
            self.io.write_all(&payload).await?;
            self.io.flush().await
        })
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "peer frame write deadline"))??;
        check_deadline(deadline)?;
        self.failed = false;
        Ok(())
    }
}
fn check_deadline(deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "peer frame deadline",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn increasing_raw_frames_preserve_original_forty_and_sixty_kib_capacity_oracle() {
        let (mut writer, reader) = tokio::io::duplex(150_000);
        let mut reader = FrameReader::new(reader, IoLimits::default()).unwrap();
        for size in [40_000usize, 60_000] {
            writer
                .write_all(&(size as u32).to_be_bytes())
                .await
                .unwrap();
            writer.write_all(&vec![b'a'; size]).await.unwrap();
            // Exercise byte framing independently of narrower application schemas.
            // The same reader must retain one buffer without geometric overshoot.
            assert_eq!(
                reader.read_with(|bytes| Ok(bytes.len())).await.unwrap(),
                Some(size)
            );
            assert!(reader.buffered_capacity() <= MAX_PEER_FRAME_BYTES);
        }
    }
}
