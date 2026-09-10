//! Persistence 辅助函数测试
//!
//! 测试 persistence/mod.rs 中的公共错误转换函数
//!
//! 本文件位于 src-tauri/tests/（集成测试），
//! 遵循 RdataStation 测试代码组织铁律。

use std::io;
use std::path::Path;

use rdata_station_lib::core::error::{CoreError, StorageError};
use rdata_station_lib::core::persistence::{
    deserialize_to_core_error, io_to_core_error, serialize_to_core_error,
};

#[test]
fn test_io_to_core_error() {
    let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
    let path = Path::new("/test/path.json");
    let core_err = io_to_core_error(io_err, path, "read");

    assert!(matches!(core_err, CoreError::Storage(_)));
    assert_eq!(core_err.code(), "STORE_IO");
}

#[test]
fn test_serialize_to_core_error() {
    let err = serialize_to_core_error("JSON", "invalid type");
    assert!(matches!(
        err,
        CoreError::Storage(StorageError::Serialization { .. })
    ));
    assert_eq!(err.code(), "STORE_SERIALIZE");
}

#[test]
fn test_deserialize_to_core_error() {
    let err = deserialize_to_core_error("JSON", "{invalid}", "unexpected token");
    assert!(matches!(
        err,
        CoreError::Storage(StorageError::Deserialization { .. })
    ));
    assert_eq!(err.code(), "STORE_DESERIALIZE");
}
