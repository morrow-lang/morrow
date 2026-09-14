//! Cancellation, deadlines and closed bounded framing at the IO boundary.
use morrow_cluster::{Frame, FrameReader, FrameWriter, IoLimits, MAX_PEER_FRAME_BYTES};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn packet(frame: &Frame) -> Vec<u8> {
    let payload = frame.encode().unwrap();
    let mut bytes = (payload.len() as u32).to_be_bytes().to_vec();
    bytes.extend(payload);
    bytes
}

#[tokio::test]
async fn peer_close_has_independent_protobuf_bytes() {
    let (writer, mut reader) = tokio::io::duplex(128);
    let mut writer = FrameWriter::new(writer, IoLimits::default()).unwrap();
    writer.write(&Frame::Close).await.unwrap();
    drop(writer);
    let mut actual = Vec::new();
    reader.read_to_end(&mut actual).await.unwrap();
    // Four-byte network length, protobuf field 1 (kind), Close = 6.
    assert_eq!(actual, [0, 0, 0, 2, 8, 6]);
}

#[tokio::test]
async fn cancelled_prefix_read_preserves_partial_header() {
    let (mut writer, reader) = tokio::io::duplex(1024);
    let mut reader = FrameReader::new(reader, IoLimits::default()).unwrap();
    let frame = Frame::Join {
        room: "snow-雪".into(),
        lease_ms: 30_000,
    };
    let bytes = packet(&frame);
    writer.write_all(&bytes[..2]).await.unwrap();
    tokio::select! {
        biased;
        value = reader.read() => panic!("incomplete prefix completed: {value:?}"),
        _ = tokio::task::yield_now() => (),
    }
    writer.write_all(&bytes[2..]).await.unwrap();
    assert_eq!(reader.read().await.unwrap(), Some(frame));
    drop(writer);
    assert_eq!(reader.read().await.unwrap(), None);
}

#[tokio::test]
async fn cancelled_body_read_resumes_exactly_once_and_preserves_next_frame() {
    let (mut writer, reader) = tokio::io::duplex(4096);
    let mut reader = FrameReader::new(reader, IoLimits::default()).unwrap();
    let first = Frame::Join {
        room: "morrow".into(),
        lease_ms: 27,
    };
    let second = Frame::Ping { nonce: 19 };
    let bytes = packet(&first);
    writer.write_all(&bytes[..9]).await.unwrap();
    tokio::select! {
        biased;
        value = reader.read() => panic!("incomplete body completed: {value:?}"),
        _ = tokio::task::yield_now() => (),
    }
    writer.write_all(&bytes[9..]).await.unwrap();
    writer.write_all(&packet(&second)).await.unwrap();
    assert_eq!(reader.read().await.unwrap(), Some(first));
    assert_eq!(reader.read().await.unwrap(), Some(second));
}

#[tokio::test]
async fn oversize_header_is_rejected_without_reading_or_allocating_its_body() {
    for size in [0, MAX_PEER_FRAME_BYTES as u32 + 1, u32::MAX] {
        let (mut writer, reader) = tokio::io::duplex(16);
        let mut reader = FrameReader::new(reader, IoLimits::default()).unwrap();
        writer.write_all(&size.to_be_bytes()).await.unwrap();
        assert_eq!(
            reader.read().await.unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        assert_eq!(reader.buffered_capacity(), 0);
    }
}

#[tokio::test(start_paused = true)]
async fn incomplete_reads_do_not_extend_the_absolute_deadline() {
    let (mut writer, reader) = tokio::io::duplex(1024);
    let limits = IoLimits {
        read_timeout: Duration::from_secs(3),
        ..Default::default()
    };
    let mut reader = FrameReader::new(reader, limits).unwrap();
    writer.write_all(&[0, 0]).await.unwrap();
    tokio::select! {
        biased;
        value = reader.read() => panic!("incomplete frame completed: {value:?}"),
        _ = tokio::task::yield_now() => (),
    }
    tokio::time::advance(Duration::from_secs(2)).await;
    writer.write_all(&[0]).await.unwrap();
    tokio::select! {
        biased;
        value = reader.read() => panic!("incomplete frame completed: {value:?}"),
        _ = tokio::task::yield_now() => (),
    }
    tokio::time::advance(Duration::from_secs(2)).await;
    assert_eq!(
        reader.read().await.unwrap_err().kind(),
        std::io::ErrorKind::TimedOut
    );
}

#[tokio::test]
async fn truncated_frame_is_not_clean_disconnect() {
    for prefix in [vec![0], vec![0, 0, 0, 7, b'{']] {
        let (mut writer, reader) = tokio::io::duplex(128);
        writer.write_all(&prefix).await.unwrap();
        drop(writer);
        let mut reader = FrameReader::new(reader, IoLimits::default()).unwrap();
        assert_eq!(
            reader.read().await.unwrap_err().kind(),
            std::io::ErrorKind::UnexpectedEof
        );
    }
}

#[tokio::test]
async fn cancelled_partial_write_cannot_be_replayed_on_the_stream() {
    let (writer, mut reader) = tokio::io::duplex(1);
    let mut writer = FrameWriter::new(writer, IoLimits::default()).unwrap();
    tokio::select! {
        biased;
        value = writer.write(&Frame::Ping { nonce: 1 }) => panic!("blocked writer completed: {value:?}"),
        _ = tokio::task::yield_now() => (),
    }
    let mut prefix = [0u8; 1];
    reader.read_exact(&mut prefix).await.unwrap();
    assert_eq!(
        writer.write(&Frame::Close).await.unwrap_err().kind(),
        std::io::ErrorKind::BrokenPipe
    );
}

#[tokio::test]
async fn closed_schema_invalid_lease_and_noncanonical_inner_integer_are_rejected() {
    for payload in [
        br#"{"type":"join","data":{"room":"room","lease_ms":0}}"#.as_slice(),
        br#"{"type":"renew","data":{"lease_ms":1}}"#.as_slice(),
        br#"{"type":"join","data":{"room":"room","lease_ms":3600001}}"#.as_slice(),
        br#"{"type":"ping","data":{"nonce":1,"secret":"ignored"}}"#.as_slice(),
        br#"{"type":"command","data":{"version":1,"incarnation":"x","namespace":"n","sequence":"01","expected_revision":"0","mutation":{"kind":"add","label":"hello"}}}"#.as_slice(),
    ] {
        let (mut writer, reader) = tokio::io::duplex(1024);
        writer.write_all(&(payload.len() as u32).to_be_bytes()).await.unwrap();
        writer.write_all(payload).await.unwrap();
        let mut reader = FrameReader::new(reader, IoLimits::default()).unwrap();
        assert_eq!(reader.read().await.unwrap_err().kind(), std::io::ErrorKind::InvalidData);
    }
}

#[tokio::test(start_paused = true)]
async fn buffered_remainder_after_deadline_is_rejected_even_when_io_is_ready() {
    let (mut writer, reader) = tokio::io::duplex(1024);
    let mut reader = FrameReader::new(
        reader,
        IoLimits {
            read_timeout: Duration::from_secs(2),
            ..Default::default()
        },
    )
    .unwrap();
    let bytes = packet(&Frame::Ping { nonce: 71 });
    writer.write_all(&bytes[..2]).await.unwrap();
    tokio::select! { biased; value=reader.read()=>panic!("incomplete {value:?}"),_=tokio::task::yield_now()=>() }
    writer.write_all(&bytes[2..]).await.unwrap();
    tokio::time::advance(Duration::from_secs(3)).await;
    assert_eq!(
        reader.read().await.unwrap_err().kind(),
        std::io::ErrorKind::TimedOut
    );
}
#[tokio::test]
async fn large_valid_snapshots_stay_within_the_frame_buffer_budget() {
    use morrow_web_protocol::{Decimal, ServerMessage, Snapshot, Task};
    let (mut writer, reader) = tokio::io::duplex(150_000);
    let mut reader = FrameReader::new(reader, IoLimits::default()).unwrap();
    for label_bytes in [200, 256] {
        let frame = Frame::Event(ServerMessage::Snapshot(Snapshot {
            version: 1,
            room: "room".into(),
            incarnation: "boot".into(),
            revision: Decimal(1),
            tasks: (1..=100)
                .map(|id| Task {
                    id: Decimal(id),
                    label: "a".repeat(label_bytes),
                    done: false,
                })
                .collect(),
        }));
        writer.write_all(&packet(&frame)).await.unwrap();
        assert_eq!(reader.read().await.unwrap(), Some(frame));
        assert!(reader.buffered_capacity() <= MAX_PEER_FRAME_BYTES);
    }
}

#[tokio::test(start_paused = true)]
async fn ready_flush_after_absolute_deadline_fails_and_poisons_writer() {
    use std::{
        cell::Cell,
        pin::Pin,
        rc::Rc,
        task::{Context, Poll},
    };
    struct Gate(Rc<Cell<bool>>);
    impl tokio::io::AsyncWrite for Gate {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            Poll::Ready(Ok(bytes.len()))
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            if self.0.get() {
                Poll::Ready(Ok(()))
            } else {
                Poll::Pending
            }
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }
    let ready = Rc::new(Cell::new(false));
    let mut writer = FrameWriter::new(
        Gate(ready.clone()),
        IoLimits {
            write_timeout: Duration::from_secs(1),
            ..Default::default()
        },
    )
    .unwrap();
    {
        let pending = writer.write(&Frame::Close);
        tokio::pin!(pending);
        tokio::select! { biased; result=&mut pending=>panic!("flush should block {result:?}"),_=tokio::task::yield_now()=>() }
        tokio::time::advance(Duration::from_secs(2)).await;
        ready.set(true);
        assert_eq!(
            pending.await.unwrap_err().kind(),
            std::io::ErrorKind::TimedOut
        );
    }
    assert_eq!(
        writer.write(&Frame::Close).await.unwrap_err().kind(),
        std::io::ErrorKind::BrokenPipe
    );
}
