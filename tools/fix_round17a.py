# -*- coding: utf-8 -*-
import pathlib

# 1) SecretError 加 Invalid 变体
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\connection\src\secret.rs")
t = p.read_text(encoding="utf-8")
old = """    #[error("Secret 不存在: {0}")]
    NotFound(String),
}"""
new = """    #[error("Secret 不存在: {0}")]
    NotFound(String),
    #[error("参数无效: {0}")]
    Invalid(String),
}"""
assert old in t
t = t.replace(old, new)
p.write_text(t, encoding="utf-8")
print("secret Invalid variant added")

# 2) secret_integration 用 Invalid
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\services\secret_integration.rs")
t = p.read_text(encoding="utf-8")
t = t.replace(
    """        return Err(connection::secret::SecretError::NotFound(format!(
            "无法解析连接 URL: {url}"
        )));""",
    """        return Err(connection::secret::SecretError::Invalid(format!(
            "无法解析连接 URL: {url}"
        )));""",
)
t = t.replace(
    """        return Err(connection::secret::SecretError::NotFound(format!(
            "不支持的 Secret 类型: {db_type}"
        )));""",
    """        return Err(connection::secret::SecretError::Invalid(format!(
            "不支持的 Secret 类型: {db_type}"
        )));""",
)
p.write_text(t, encoding="utf-8")
print("secret_integration Invalid used")
