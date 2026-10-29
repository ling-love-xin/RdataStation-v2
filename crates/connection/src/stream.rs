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

    #[tokio::test]
    async fn test_stream_method_name_and_addressing() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // TCP 变体：地址可取、非加密
        let client = tokio::net::TcpStream::connect(addr).await.unwrap();
        let stream = ConnectionStream::tcp(client);
        assert_eq!(stream.method_name(), "tcp");
        assert!(!stream.is_encrypted());
        assert!(stream.local_addr().is_ok(), "TCP 本地地址可取");
        assert!(stream.peer_addr().is_ok(), "TCP 对端地址可取");

        // SSH 隧道变体（本地转发流）：同样可取地址
        let client2 = tokio::net::TcpStream::connect(addr).await.unwrap();
        let stream2 = ConnectionStream::ssh_tunnel(client2);
        assert_eq!(stream2.method_name(), "ssh_tunnel");
        assert!(stream2.local_addr().is_ok());
        assert!(!stream2.is_encrypted());
    }
}
