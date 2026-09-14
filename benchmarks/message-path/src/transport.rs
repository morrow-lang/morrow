use crate::{Codec, Result, ns};
use fern_network_codecs::{Message, decode, encode};
use fern_web_protocol::MAX_FRAME_BYTES;
use std::{
    net::{SocketAddr, TcpStream},
    time::{Duration, Instant},
};
use tungstenite::{Message as WsMessage, WebSocket, protocol::WebSocketConfig};
pub type Socket = WebSocket<TcpStream>;
fn config() -> WebSocketConfig {
    WebSocketConfig::default()
        .read_buffer_size(MAX_FRAME_BYTES)
        .write_buffer_size(MAX_FRAME_BYTES)
        .max_write_buffer_size(MAX_FRAME_BYTES * 3)
        .max_message_size(Some(MAX_FRAME_BYTES))
        .max_frame_size(Some(MAX_FRAME_BYTES))
}
fn setup(stream: &TcpStream) -> Result<()> {
    stream.set_nodelay(true).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())
}
pub fn connect(address: SocketAddr) -> Result<TcpStream> {
    let stream =
        TcpStream::connect_timeout(&address, Duration::from_secs(5)).map_err(|e| e.to_string())?;
    setup(&stream)?;
    Ok(stream)
}
pub fn client(stream: TcpStream, address: SocketAddr) -> Result<Socket> {
    tungstenite::client::client_with_config(
        format!("ws://{address}/measurement"),
        stream,
        Some(config()),
    )
    .map(|(socket, _)| socket)
    .map_err(|e| e.to_string())
}
pub fn accept(stream: TcpStream) -> Result<Socket> {
    setup(&stream)?;
    tungstenite::accept_with_config(stream, Some(config())).map_err(|e| e.to_string())
}
pub fn encoded(codec: Codec, message: &Message) -> Result<WsMessage> {
    let bytes = encode(codec.into(), message)?;
    match codec {
        Codec::Json => String::from_utf8(bytes)
            .map(|text| WsMessage::Text(text.into()))
            .map_err(|e| e.to_string()),
        _ => Ok(WsMessage::Binary(bytes.into())),
    }
}
pub fn send(socket: &mut Socket, codec: Codec, message: &Message) -> Result<()> {
    socket
        .send(encoded(codec, message)?)
        .map_err(|e| e.to_string())
}
pub fn read(
    socket: &mut Socket,
    codec: Codec,
    client_message: bool,
) -> Result<(Message, u64, usize)> {
    let message = socket.read().map_err(|e| e.to_string())?;
    let bytes = match (codec, &message) {
        (Codec::Json, WsMessage::Text(text)) => text.as_bytes(),
        (Codec::Cbor | Codec::Protobuf, WsMessage::Binary(bytes)) => bytes.as_ref(),
        _ => return Err("wrong WebSocket message kind".into()),
    };
    let start = Instant::now();
    let decoded = decode(codec.into(), bytes, client_message)?;
    Ok((decoded, ns(start), bytes.len()))
}
