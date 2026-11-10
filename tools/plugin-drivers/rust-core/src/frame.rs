use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// §4.2：单帧上限 64 MiB（含头）。
pub const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;
const HEADER_LEN: usize = 5;
pub const KIND_JSON: u8 = 0x01;
pub const KIND_ARROW: u8 = 0x02;

#[derive(Debug, Clone)]
pub struct Frame {
    pub kind: u8,
    pub payload: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("帧长度非法：total_len={total}（至少 {HEADER_LEN}，上限 {MAX_FRAME_BYTES}）")]
    BadLength { total: usize },
    #[error("未知帧种类：0x{kind:02x}")]
    UnknownKind { kind: u8 },
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub struct FrameReader<R> {
    inner: R,
}

impl<R: AsyncRead + Unpin> FrameReader<R> {
    pub fn new(inner: R) -> Self {
        Self { inner }
    }

    /// 读一帧。返回 `None` 表示 stdin 干净 EOF（宿主已退出）。
    pub async fn read_frame(&mut self) -> Result<Option<Frame>, FrameError> {
        let mut header = [0u8; HEADER_LEN];
        // 先读 1 字节，才能把「干净 EOF」和「截断帧」区分开。
        let first = self.inner.read(&mut header[..1]).await?;
        if first == 0 {
            return Ok(None);
        }
        self.inner.read_exact(&mut header[1..]).await?;
        let total = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;
        if !(HEADER_LEN..=MAX_FRAME_BYTES).contains(&total) {
            return Err(FrameError::BadLength { total });
        }
        let kind = header[4];
        if kind != KIND_JSON && kind != KIND_ARROW {
            return Err(FrameError::UnknownKind { kind });
        }
        let mut payload = vec![0u8; total - HEADER_LEN];
        self.inner.read_exact(&mut payload).await?;
        Ok(Some(Frame { kind, payload }))
    }
}

pub struct FrameWriter<W> {
    inner: W,
}

impl<W: AsyncWrite + Unpin> FrameWriter<W> {
    pub fn new(inner: W) -> Self {
        Self { inner }
    }

    /// 写单帧并 flush。
    pub async fn write_frame(&mut self, kind: u8, payload: &[u8]) -> Result<(), io::Error> {
        let total = HEADER_LEN + payload.len();
        if total > MAX_FRAME_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("帧超过 {MAX_FRAME_BYTES} 字节"),
            ));
        }
        let mut buf = Vec::with_capacity(total);
        buf.extend_from_slice(&(total as u32).to_be_bytes());
        buf.push(kind);
        buf.extend_from_slice(payload);
        self.inner.write_all(&buf).await?;
        self.inner.flush().await
    }

    /// 响应头 + Arrow 分片必须**一次原子写**，否则宿主附件装配错位。
    pub async fn write_json_with_arrow(
        &mut self,
        json_payload: &[u8],
        chunks: &[Vec<u8>],
    ) -> Result<(), io::Error> {
        let mut frames: Vec<(u8, &[u8])> = Vec::with_capacity(chunks.len() + 1);
        frames.push((KIND_JSON, json_payload));
        for chunk in chunks {
            frames.push((KIND_ARROW, chunk.as_slice()));
        }
        let total: usize = frames.iter().map(|(_, p)| HEADER_LEN + p.len()).sum();
        if total > MAX_FRAME_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "响应与附件合计超过单帧上限（应减少每页行数）",
            ));
        }
        let mut buf = Vec::with_capacity(total);
        for (kind, payload) in frames {
            let frame_len = HEADER_LEN + payload.len();
            buf.extend_from_slice(&(frame_len as u32).to_be_bytes());
            buf.push(kind);
            buf.extend_from_slice(payload);
        }
        self.inner.write_all(&buf).await?;
        self.inner.flush().await
    }
}
