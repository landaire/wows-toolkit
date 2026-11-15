use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;
use tracing::debug;
use tracing::error;
use tracing::warn;
use wgc_relay_protocol::FRAME_HEADER_LEN;
use wgc_relay_protocol::encode_frame_header;
use wgc_relay_protocol::parse_frame_header;
use zeroize::Zeroizing;

use crate::answer::answer;
use crate::error::IpcError;
use crate::message::Message;
use crate::responder::IpcResponder;

/// Answers one pipe message. A request the codec cannot model (an unknown
/// command type, or fields of a newer class version) is echoed unchanged, the
/// reply that leaves every output at the client's own default. The reply may
/// carry tokens.
pub async fn handle_message(request: &[u8], responder: &dyn IpcResponder) -> Zeroizing<Vec<u8>> {
    let mut message = match Message::decode(request) {
        Ok(message) => message,
        Err(err) => {
            warn!(%err, len = request.len(), "unmodeled WGC IPC request, echoing it");
            return Zeroizing::new(request.to_vec());
        }
    };

    // Debug output redacts tokens.
    debug!(?message, "WGC IPC request");
    for frame in &mut message.commands {
        answer(frame, responder).await;
    }
    debug!(?message, "WGC IPC reply");

    match message.encode() {
        Ok(reply) => reply,
        Err(err) => {
            error!(%err, "answered WGC IPC message does not encode, echoing the request");
            Zeroizing::new(request.to_vec())
        }
    }
}

/// Serves length-prefixed WGC IPC messages until the peer closes the stream.
pub async fn serve_stream<S>(mut stream: S, responder: &dyn IpcResponder) -> Result<(), IpcError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    loop {
        let Some(request) = read_frame(&mut stream).await? else {
            return Ok(());
        };

        let reply = handle_message(&request, responder).await;

        stream.write_all(&encode_frame_header(reply.len())?).await?;
        stream.write_all(&reply).await?;
        stream.flush().await?;
    }
}

/// A request may carry a token the game saves.
async fn read_frame<S>(stream: &mut S) -> Result<Option<Zeroizing<Vec<u8>>>, IpcError>
where
    S: AsyncRead + Unpin,
{
    let mut header = [0u8; FRAME_HEADER_LEN];
    let first = stream.read(&mut header).await?;
    if first == 0 {
        return Ok(None);
    }
    stream.read_exact(&mut header[first..]).await?;

    let mut payload = Zeroizing::new(vec![0u8; parse_frame_header(header)?]);
    stream.read_exact(&mut payload).await?;

    Ok(Some(payload))
}
