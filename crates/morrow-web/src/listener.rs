//! Admission starts at accepted TCP streams, before HTTP parsing or upgrade.
use std::{
    future::Future,
    io,
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::{TcpListener, TcpStream},
    sync::{OwnedSemaphorePermit, Semaphore},
    time::{Instant, Sleep},
};

/// Caps live accepted streams including idle clients and partial HTTP handshakes.
pub struct BoundedListener {
    listener: TcpListener,
    permits: Arc<Semaphore>,
    handshake_timeout: Duration,
}
impl BoundedListener {
    pub fn new(
        listener: TcpListener,
        maximum: usize,
        handshake_timeout: Duration,
    ) -> io::Result<Self> {
        if !(1..=2048).contains(&maximum)
            || handshake_timeout.is_zero()
            || handshake_timeout > Duration::from_secs(30)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid TCP admission limits",
            ));
        }
        Ok(Self {
            listener,
            permits: Arc::new(Semaphore::new(maximum)),
            handshake_timeout,
        })
    }
}
impl axum::serve::Listener for BoundedListener {
    type Io = Connection;
    type Addr = SocketAddr;
    async fn accept(&mut self) -> (Connection, SocketAddr) {
        loop {
            match self.listener.accept().await {
                Ok((stream, address)) => {
                    if let Ok(permit) = self.permits.clone().try_acquire_owned() {
                        let _ = stream.set_nodelay(true);
                        return (
                            Connection {
                                stream,
                                _permit: permit,
                                deadline: Box::pin(tokio::time::sleep(self.handshake_timeout)),
                                header_bytes: 0,
                                delimiter: 0,
                                handshaking: true,
                            },
                            address,
                        );
                    }
                    // Drop rejected sockets immediately; never queue an unbounded
                    // future waiting for a permit while retaining its descriptor.
                    drop(stream);
                    tokio::task::yield_now().await;
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    }
    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }
}
/// Owns the admission permit until the HTTP or upgraded WebSocket stream closes.
pub struct Connection {
    stream: TcpStream,
    _permit: OwnedSemaphorePermit,
    deadline: Pin<Box<Sleep>>,
    header_bytes: usize,
    delimiter: usize,
    handshaking: bool,
}
impl AsyncRead for Connection {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.deadline.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "connection read deadline",
            )));
        }
        let before = buffer.filled().len();
        match Pin::new(&mut this.stream).poll_read(cx, buffer) {
            Poll::Ready(Ok(())) => {
                if buffer.filled().len() > before {
                    if this.handshaking {
                        for &byte in &buffer.filled()[before..] {
                            this.header_bytes += 1;
                            if this.header_bytes > 16 * 1024 {
                                // AsyncRead may not report new filled bytes with
                                // an error, even though the underlying TCP read did.
                                buffer.set_filled(before);
                                return Poll::Ready(Err(io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    "HTTP headers exceed 16 KiB",
                                )));
                            }
                            this.delimiter = match (this.delimiter, byte) {
                                (0 | 2, b'\r') => this.delimiter + 1,
                                (1 | 3, b'\n') => this.delimiter + 1,
                                (_, b'\r') => 1,
                                _ => 0,
                            };
                            if this.delimiter == 4 {
                                this.handshaking = false;
                                break;
                            }
                        }
                    }
                    if !this.handshaking {
                        this.deadline
                            .as_mut()
                            .reset(Instant::now() + Duration::from_secs(60));
                    }
                }
                Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}
impl AsyncWrite for Connection {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().stream).poll_write(cx, bytes)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::serve::Listener;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[tokio::test]
    async fn idle_tcp_admission_rejects_excess_and_releases_permit() {
        let socket = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = socket.local_addr().unwrap();
        let mut listener = BoundedListener::new(socket, 1, Duration::from_secs(1)).unwrap();
        let _first = TcpStream::connect(address).await.unwrap();
        let (accepted, _) = listener.accept().await;
        assert_eq!(listener.permits.available_permits(), 0);
        let mut second = TcpStream::connect(address).await.unwrap();
        let accept = tokio::spawn(async move {
            let result = listener.accept().await;
            (listener, result)
        });
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), second.read(&mut [0]))
                .await
                .unwrap()
                .unwrap(),
            0
        );
        drop(accepted);
        let _third = TcpStream::connect(address).await.unwrap();
        let (listener, (accepted, _)) = tokio::time::timeout(Duration::from_secs(1), accept)
            .await
            .unwrap()
            .unwrap();
        drop(accepted);
        assert_eq!(listener.permits.available_permits(), 1);
    }
    #[tokio::test]
    async fn partial_header_has_absolute_deadline_and_size_cap() {
        let socket = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = socket.local_addr().unwrap();
        let mut listener = BoundedListener::new(socket, 1, Duration::from_millis(20)).unwrap();
        let mut client = TcpStream::connect(address).await.unwrap();
        let (mut accepted, _) = listener.accept().await;
        client.write_all(b"GET / HTTP/1.1\r\n").await.unwrap();
        let mut bytes = [0; 128];
        assert!(accepted.read(&mut bytes).await.unwrap() > 0);
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), accepted.read(&mut bytes))
                .await
                .unwrap()
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        drop(accepted);
        listener.handshake_timeout = Duration::from_secs(1);
        let mut client = TcpStream::connect(address).await.unwrap();
        let (mut accepted, _) = listener.accept().await;
        client.write_all(&vec![b'x'; 16 * 1024 + 1]).await.unwrap();
        let mut buffer = Vec::new();
        assert_eq!(
            accepted.read_to_end(&mut buffer).await.unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
