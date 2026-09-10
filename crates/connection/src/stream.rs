//! 连接流抽象
//!
//! 提供统一的异步流接口，包装不同类型的底层连接

use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// 连接流包装器
///
/// 统一包装不同类型的连接流：
/// - 普通 TCP 流
/// - TLS 加密流
/// - SSH 隧道流
/// - 代理连接流
pub enum ConnectionStream {
    /// 普通 TCP 连接
    Tcp(tokio::net::TcpStream),
    /// TLS 加密连接
    Tls(Box<tokio_native_tls::TlsStream<tokio::net::TcpStream>>),
    /// SSH 隧道连接（本地端口转发）
    SshTunnel(tokio::net::TcpStream),
    /// HTTP 代理连接
    HttpProxy(tokio::net::TcpStream),
    /// SOCKS 代理连接
    SocksProxy(tokio::net::TcpStream),
}

impl AsyncRead for ConnectionStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            ConnectionStream::Tcp(stream) => Pin::new(stream).poll_read(cx, buf),
            ConnectionStream::Tls(stream) => Pin::new(stream).poll_read(cx, buf),
            ConnectionStream::SshTunnel(stream) => Pin::new(stream).poll_read(cx, buf),
            ConnectionStream::HttpProxy(stream) => Pin::new(stream).poll_read(cx, buf),
            ConnectionStream::SocksProxy(stream) => Pin::new(stream).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for ConnectionStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            ConnectionStream::Tcp(stream) => Pin::new(stream).poll_write(cx, buf),
            ConnectionStream::Tls(stream) => Pin::new(stream).poll_write(cx, buf),
            ConnectionStream::SshTunnel(stream) => Pin::new(stream).poll_write(cx, buf),
            ConnectionStream::HttpProxy(stream) => Pin::new(stream).poll_write(cx, buf),
            ConnectionStream::SocksProxy(stream) => Pin::new(stream).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            ConnectionStream::Tcp(stream) => Pin::new(stream).poll_flush(cx),
            ConnectionStream::Tls(stream) => Pin::new(stream).poll_flush(cx),
            ConnectionStream::SshTunnel(stream) => Pin::new(stream).poll_flush(cx),
            ConnectionStream::HttpProxy(stream) => Pin::new(stream).poll_flush(cx),
            ConnectionStream::SocksProxy(stream) => Pin::new(stream).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            ConnectionStream::Tcp(stream) => Pin::new(stream).poll_shutdown(cx),
            ConnectionStream::Tls(stream) => Pin::new(stream).poll_shutdown(cx),
            ConnectionStream::SshTunnel(stream) => Pin::new(stream).poll_shutdown(cx),
            ConnectionStream::HttpProxy(stream) => Pin::new(stream).poll_shutdown(cx),
            ConnectionStream::SocksProxy(stream) => Pin::new(stream).poll_shutdown(cx),
        }
    }
}

/// 流包装器 trait
///
/// 用于在流上添加额外的功能（如日志、监控等）
pub trait StreamWrapper: AsyncRead + AsyncWrite + Send + Unpin {
    /// 获取流的元信息
    fn stream_info(&self) -> StreamInfo;
}

/// 流信息
#[derive(Debug, Clone)]
pub struct StreamInfo {
    /// 连接方式
    pub method: String,
    /// 本地地址
    pub local_addr: String,
    /// 远程地址
    pub remote_addr: String,
    /// 是否加密
    pub is_encrypted: bool,
    /// 连接建立时间
    pub established_at: std::time::Instant,
}

impl ConnectionStream {
    /// 创建 TCP 连接流
    pub fn tcp(stream: tokio::net::TcpStream) -> Self {
        ConnectionStream::Tcp(stream)
    }

    /// 创建 TLS 连接流
    pub fn tls(stream: tokio_native_tls::TlsStream<tokio::net::TcpStream>) -> Self {
        ConnectionStream::Tls(Box::new(stream))
    }

    /// 创建 SSH 隧道连接流
    pub fn ssh_tunnel(stream: tokio::net::TcpStream) -> Self {
        ConnectionStream::SshTunnel(stream)
    }

    /// 尝试获取本地地址
    ///
    /// 注意：对于 TLS 连接，此方法可能不可用
    pub fn local_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        match self {
            ConnectionStream::Tcp(s) => s.local_addr(),
            ConnectionStream::Tls(_) => {
                // TLS 流的地址获取比较复杂，暂时返回一个默认值
                // 实际使用时可以通过其他方式获取
                Err(std::io::Error::other(
                    "TLS stream local address not available",
                ))
            }
            ConnectionStream::SshTunnel(s) => s.local_addr(),
            ConnectionStream::HttpProxy(s) => s.local_addr(),
            ConnectionStream::SocksProxy(s) => s.local_addr(),
        }
    }

    /// 尝试获取远程地址
    ///
    /// 注意：对于 TLS 连接，此方法可能不可用
    pub fn peer_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        match self {
            ConnectionStream::Tcp(s) => s.peer_addr(),
            ConnectionStream::Tls(_) => {
                // TLS 流的地址获取比较复杂，暂时返回一个默认值
                Err(std::io::Error::other(
                    "TLS stream peer address not available",
                ))
            }
            ConnectionStream::SshTunnel(s) => s.peer_addr(),
            ConnectionStream::HttpProxy(s) => s.peer_addr(),
            ConnectionStream::SocksProxy(s) => s.peer_addr(),
        }
    }

    /// 检查是否为加密连接
    pub fn is_encrypted(&self) -> bool {
        matches!(self, ConnectionStream::Tls(_))
    }

    /// 获取连接方式名称
    pub fn method_name(&self) -> &'static str {
        match self {
            ConnectionStream::Tcp(_) => "tcp",
            ConnectionStream::Tls(_) => "tls",
            ConnectionStream::SshTunnel(_) => "ssh_tunnel",
            ConnectionStream::HttpProxy(_) => "http_proxy",
            ConnectionStream::SocksProxy(_) => "socks_proxy",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_info() {
        let info = StreamInfo {
            method: "tcp".to_string(),
            local_addr: "127.0.0.1:1234".to_string(),
            remote_addr: "127.0.0.1:3306".to_string(),
            is_encrypted: false,
            established_at: std::time::Instant::now(),
        };
        assert_eq!(info.method, "tcp");
        assert!(!info.is_encrypted);
    }
}
