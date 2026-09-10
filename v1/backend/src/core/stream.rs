use crate::core::arrow::ArrowBatch;

/// 通用流式 trait
#[async_trait::async_trait]
pub trait Stream: Send + Sync {
    /// 尝试获取下一个批次
    async fn next(&mut self) -> Option<Result<ArrowBatch, crate::core::error::CoreError>>;

    /// 关闭流
    async fn close(&mut self);
}

/// Arrow 批处理流实现
pub struct ArrowBatchStream {
    receiver: tokio::sync::mpsc::Receiver<Result<ArrowBatch, crate::core::error::CoreError>>,
}

impl ArrowBatchStream {
    /// 创建新的 Arrow 批处理流
    pub fn new(
        receiver: tokio::sync::mpsc::Receiver<Result<ArrowBatch, crate::core::error::CoreError>>,
    ) -> Self {
        Self { receiver }
    }
}

#[async_trait::async_trait]
impl Stream for ArrowBatchStream {
    async fn next(&mut self) -> Option<Result<ArrowBatch, crate::core::error::CoreError>> {
        self.receiver.recv().await
    }

    async fn close(&mut self) {
        // 关闭接收端 - 对于mpsc::Receiver，当它被drop时会自动关闭
        // 不需要显式drop，因为我们没有所有权
    }
}

/// 流式查询结果
pub struct StreamQueryResult {
    pub stream: Box<dyn Stream>,
    pub schema: arrow::datatypes::SchemaRef,
}

impl StreamQueryResult {
    /// 创建新的流式查询结果
    pub fn new(stream: Box<dyn Stream>, schema: arrow::datatypes::SchemaRef) -> Self {
        Self { stream, schema }
    }
}
