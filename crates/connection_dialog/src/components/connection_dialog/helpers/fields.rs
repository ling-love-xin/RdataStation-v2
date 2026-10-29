use super::*;

// ===== 驱动声明的连接字段（`drivers.config_schema`）=====
//
// 常规 Tab 的字段集合是「驱动 schema × 连接方式」的函数：
// - 行的**存在性**看 schema 是否声明了该键（如某驱动无 database 字段就不出该行）；
// - 行的**标签 / 占位**取 schema 声明（不再写死「主机 / 端口 / 数据库」）；
// - 未声明的字段不造默认行（见架构 §15）。

/// 驱动声明的连接字段（`config_schema.fields[]` 子集）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FormField {
    pub key: String,
    pub label: String,
    /// `text` / `number` / `password` / `file` / `select` / `textarea` / `bool`（未声明 → `text`）。
    pub kind: String,
    pub required: bool,
    pub placeholder: Option<String>,
}

/// 解析 `drivers.config_schema` 的 `fields`（非法 / 缺字段 → 空列表；不造默认值）。
pub(crate) fn driver_form_fields(config_schema: &str) -> Vec<FormField> {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(config_schema) else {
        return Vec::new();
    };
    let Some(list) = json.get("fields").and_then(|f| f.as_array()) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|f| {
            let key = f.get("key")?.as_str()?.trim();
            if key.is_empty() {
                return None;
            }
            Some(FormField {
                key: key.to_string(),
                label: f
                    .get("label")
                    .and_then(|x| x.as_str())
                    .unwrap_or(key)
                    .to_string(),
                kind: f
                    .get("type")
                    .and_then(|x| x.as_str())
                    .unwrap_or("text")
                    .to_string(),
                required: f.get("required").and_then(|x| x.as_bool()).unwrap_or(false),
                placeholder: f
                    .get("placeholder")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string()),
            })
        })
        .collect()
}

/// 按键查驱动声明字段（未声明 → `None`）。
pub(crate) fn field_spec<'a>(fields: &'a [FormField], key: &str) -> Option<&'a FormField> {
    fields.iter().find(|f| f.key == key)
}

/// 驱动派生数据：渲染期反复用到的三份声明解析结果（表单字段 / 能力 / 认证方法）。
///
/// 只做数据聚合；缓存与失效判别在 [`ConnectionDialogState::driver_derived`]。
/// `key` 取驱动 id + 三段声明原文（声明变了即失效），避免为拿键而先解析 JSON。
///
/// [`ConnectionDialogState::driver_derived`]: crate::components::connection_dialog::ConnectionDialogState::driver_derived
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DriverDerived {
    /// 缓存键：驱动 id + 三段声明原文；无驱动时为空串。
    pub(crate) key: String,
    pub(crate) form_fields: Vec<FormField>,
    pub(crate) capabilities: Vec<String>,
    pub(crate) auth_types: Vec<String>,
}

impl DriverDerived {
    /// 缓存键（只拼接字符串，不解析 JSON）。
    pub(crate) fn cache_key(driver: &Driver) -> String {
        format!(
            "{}|{}|{}|{}",
            driver.id,
            driver.config_schema,
            driver.capabilities.as_deref().unwrap_or(""),
            driver.supported_auth_types.as_deref().unwrap_or("")
        )
    }

    /// 由驱动目录行解析（`None` → 全空，与“未选驱动”语义一致）。
    pub(crate) fn resolve(driver: Option<&Driver>) -> Self {
        let Some(d) = driver else {
            return Self::default();
        };
        Self {
            key: Self::cache_key(d),
            form_fields: driver_form_fields(&d.config_schema),
            capabilities: driver_capabilities(d.capabilities.as_deref()),
            auth_types: driver_auth_types(d.supported_auth_types.as_deref()),
        }
    }
}

/// 地址字段：优先 `type = file`，否则按常见键名（`file_path` / `path` / `file`）。
pub(crate) fn address_field(fields: &[FormField]) -> Option<&FormField> {
    fields.iter().find(|f| f.kind == "file").or_else(|| {
        fields
            .iter()
            .find(|f| matches!(f.key.as_str(), "file_path" | "path" | "file"))
    })
}

// ===== 网络配置字段（`network_configs.config` 的结构化编辑）=====
//
// 背景（审计 #20 / #24）：管理器原先只有一个「数据(JSON)」文本框，用户必须手写
// `SshConfig` / `ProxyConfig` 的 JSON 才能建出可用档案（写错就叫苦不迭）。
// 这里把字段声明、JSON 组装、校验与回填都做成纯函数，渲染层只负责把输入框摆出来——
// 与「驱动 schema → 表单」同一思路（架构 §15：UI 不造数据，只管形状）。

/// 网络配置字段的输入形态（渲染层据此选 Input 与占位文案）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NetFieldKind {
    Text,
    Port,
    Password,
    Bool,
}

/// 单个字段声明：`path` 是写入 config JSON 的路径（多段 → 嵌套对象）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NetFieldSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub path: &'static [&'static str],
    pub kind: NetFieldKind,
    pub required: bool,
    pub placeholder: &'static str,
    /// true = 逗号分隔 → JSON 数组（如 `no_proxy`）。
    pub array: bool,
}

fn net_spec(
    key: &'static str,
    label: &'static str,
    path: &'static [&'static str],
    kind: NetFieldKind,
    required: bool,
    placeholder: &'static str,
) -> NetFieldSpec {
    NetFieldSpec {
        key,
        label,
        path,
        kind,
        required,
        placeholder,
        array: false,
    }
}

/// 某类型需要显示的字段（**空 = 走原始 JSON 文本框**，如协议链）。
///
/// 字段与 `connection::config` 的 serde 模型一一对应：
/// - `proxy` / `socks`：`ProxyConfig`（host / port / auth.username / auth.password / no_proxy）；
/// - `ssh`：`SshConfig`（host / port / username / password 或 key_path / remote_host / remote_port）；
/// - `ssl`：`SslConfig`（verify_server_cert / 三个证书路径）。
pub(crate) fn network_field_specs(type_key: &str) -> Vec<NetFieldSpec> {
    let port = NetFieldKind::Port;
    match type_key.trim().to_ascii_lowercase().as_str() {
        "ssh" | "ssh_tunnel" => vec![
            net_spec(
                "host",
                tr("SSH 主机"),
                &["host"],
                NetFieldKind::Text,
                true,
                "jump.example.com",
            ),
            net_spec("port", tr("SSH 端口"), &["port"], port, false, "22"),
            net_spec(
                "username",
                tr("SSH 用户名"),
                &["username"],
                NetFieldKind::Text,
                true,
                "deploy",
            ),
            net_spec(
                "password",
                tr("SSH 密码"),
                &["password"],
                NetFieldKind::Password,
                false,
                tr("与私钥二选一"),
            ),
            net_spec(
                "key_path",
                tr("私钥路径"),
                &["key_path"],
                NetFieldKind::Text,
                false,
                "~/.ssh/id_ed25519",
            ),
            net_spec(
                "remote_host",
                tr("目标主机"),
                &["remote_host"],
                NetFieldKind::Text,
                true,
                tr("数据库主机（隧道出口）"),
            ),
            net_spec(
                "remote_port",
                tr("目标端口"),
                &["remote_port"],
                port,
                true,
                "5432",
            ),
        ],
        "proxy" | "http" | "http_proxy" => {
            let mut specs = proxy_field_specs("8080");
            specs[1].placeholder = "8080";
            specs
        }
        "socks" | "socks5" | "socks_proxy" => {
            let mut specs = proxy_field_specs("1080");
            specs[1].placeholder = "1080";
            specs
        }
        "ssl" | "tls" => vec![
            net_spec(
                "verify",
                tr("校验服务器证书"),
                &["verify_server_cert"],
                NetFieldKind::Bool,
                false,
                "true",
            ),
            net_spec(
                "ca",
                tr("CA 证书路径"),
                &["ca_cert_path"],
                NetFieldKind::Text,
                false,
                "/etc/ssl/ca.pem",
            ),
            net_spec(
                "cert",
                tr("客户端证书"),
                &["client_cert_path"],
                NetFieldKind::Text,
                false,
                tr("（可选）"),
            ),
            net_spec(
                "key",
                tr("客户端私钥"),
                &["client_key_path"],
                NetFieldKind::Text,
                false,
                tr("（可选）"),
            ),
        ],
        _ => Vec::new(),
    }
}

fn proxy_field_specs(default_port: &'static str) -> Vec<NetFieldSpec> {
    vec![
        net_spec(
            "host",
            tr("代理主机"),
            &["host"],
            NetFieldKind::Text,
            true,
            "127.0.0.1",
        ),
        net_spec(
            "port",
            tr("代理端口"),
            &["port"],
            NetFieldKind::Port,
            true,
            default_port,
        ),
        net_spec(
            "username",
            tr("代理用户名"),
            &["auth", "username"],
            NetFieldKind::Text,
            false,
            tr("（可选）"),
        ),
        net_spec(
            "password",
            tr("代理密码"),
            &["auth", "password"],
            NetFieldKind::Password,
            false,
            tr("（可选）"),
        ),
        NetFieldSpec {
            key: "no_proxy",
            label: tr("直连主机"),
            path: &["no_proxy"],
            kind: NetFieldKind::Text,
            required: false,
            placeholder: "localhost,127.0.0.1",
            array: true,
        },
    ]
}

/// 字段值（key → 原始文本）→ config JSON（校验必填 / 端口 / 布尔）。
///
/// SSH 认证方式：填了私钥路径→ `auth_type=private_key`，否则用密码；两者都空报错。
pub(crate) fn build_network_config_json(
    network_type: &str,
    values: &[(String, String)],
) -> Result<String, String> {
    let specs = network_field_specs(network_type);
    if specs.is_empty() {
        return Err(format!(
            "类型「{}」不支持字段编辑（协议链请选 chain 并直接填 JSON）",
            network_type.trim()
        ));
    }
    let get = |key: &str| -> String {
        values
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.trim().to_string())
            .unwrap_or_default()
    };

    for spec in &specs {
        let raw = get(spec.key);
        if spec.required && raw.is_empty() {
            return Err(format!("「{}」为必填项", spec.label));
        }
        if spec.kind == NetFieldKind::Port && !raw.is_empty() && raw.parse::<u16>().is_err() {
            return Err(format!("「{}」需要 1-65535 的端口号", spec.label));
        }
        if spec.kind == NetFieldKind::Bool && !raw.is_empty() && !is_bool_text(&raw) {
            return Err(format!("「{}」需要 true / false", spec.label));
        }
    }

    let mut root = serde_json::Map::new();
    for spec in &specs {
        let raw = get(spec.key);
        if raw.is_empty() {
            continue;
        }
        let value = if spec.array {
            serde_json::Value::Array(
                raw.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| serde_json::Value::String(s.to_string()))
                    .collect(),
            )
        } else {
            match spec.kind {
                NetFieldKind::Port => serde_json::Value::Number(
                    raw.parse::<u16>()
                        .map_err(|_| format!("「{}」端口非法", spec.label))?
                        .into(),
                ),
                NetFieldKind::Bool => serde_json::Value::Bool(parse_bool_text(&raw)),
                _ => serde_json::Value::String(raw),
            }
        };
        insert_json_path(&mut root, spec.path, value);
    }

    // SSH 的 `auth` 是内部标签枚举（`auth_type`），配置里必须显式给出。
    if network_type.trim().eq_ignore_ascii_case("ssh")
        || network_type.trim().eq_ignore_ascii_case("ssh_tunnel")
    {
        let key_path = get("key_path");
        let password = get("password");
        if !key_path.is_empty() {
            root.insert(
                "auth_type".into(),
                serde_json::Value::String("private_key".into()),
            );
        } else if !password.is_empty() {
            root.insert(
                "auth_type".into(),
                serde_json::Value::String("password".into()),
            );
        } else {
            return Err("SSH 需要「密码」或「私钥路径」二者之一".into());
        }
    }

    Ok(serde_json::Value::Object(root).to_string())
}

/// config JSON → 字段值（编辑回填；缺失 / 非法 JSON → 全空，不报错）。
pub(crate) fn network_config_values(
    network_type: &str,
    config_json: &str,
) -> Vec<(String, String)> {
    spec_field_values(network_field_specs(network_type), config_json)
}

/// 按字段声明从 JSON 回填文本值（网络 / 认证共用）。
fn spec_field_values(specs: Vec<NetFieldSpec>, json: &str) -> Vec<(String, String)> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    specs
        .into_iter()
        .map(|spec| {
            let text = match read_json_path(&value, spec.path) {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(serde_json::Value::Number(n)) => n.to_string(),
                Some(serde_json::Value::Bool(b)) => b.to_string(),
                Some(serde_json::Value::Array(items)) => items
                    .iter()
                    .filter_map(|i| i.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
                _ => String::new(),
            };
            (spec.key.to_string(), text)
        })
        .collect()
}

// ===== 认证配置字段（`auth_configs.auth_data` 的结构化编辑）=====
//
// 背景（审计 A5）：认证管理器原先只有「数据(JSON)」文本框，用户必须手写
// `inject_auth_into_url` / `inject_{ssh,proxy}_auth_from_auth_data` 认得的键（含
// camelCase 的 `keyPath` / `certPath` / `keytabPath`）。与网络档案同款：字段声明 +
// JSON 组装 / 校验 / 回填都是纯函数，渲染层只摆输入框。

/// 某认证类型需要显示的字段（**空 = 走原始 JSON 文本框**）。
///
/// `path` 是**写进 `auth_data` 的键**，与后端读取端严格一致：
/// - `password` / `ldap`：`username` / `password`（`inject_auth_into_url` 同一分支）；
/// - `ssh_key`：`keyPath` / `passphrase` / `password`（`inject_ssh_auth_from_auth_data`）；
/// - `proxy_pwd`：`username` / `password`（`inject_proxy_auth_from_auth_data`）；
/// - `pg_class`：`certPath` / `certKeyPath`；`kerberos`：`principal` / `keytabPath`。
pub(crate) fn auth_field_specs(auth_type: &str) -> Vec<NetFieldSpec> {
    match auth_type.trim().to_ascii_lowercase().as_str() {
        "password" | "ldap" => vec![
            net_spec(
                "username",
                tr("用户名"),
                &["username"],
                NetFieldKind::Text,
                true,
                tr("数据库账号"),
            ),
            net_spec(
                "password",
                tr("密码"),
                &["password"],
                NetFieldKind::Password,
                true,
                tr("登录密码"),
            ),
        ],
        "ssh_key" | "ssh" => vec![
            net_spec(
                "username",
                tr("SSH 用户名"),
                &["username"],
                NetFieldKind::Text,
                false,
                tr("（可选）覆盖档案里的用户名"),
            ),
            net_spec(
                "key_path",
                tr("私钥路径"),
                &["keyPath"],
                NetFieldKind::Text,
                false,
                tr("与密码二选一"),
            ),
            net_spec(
                "passphrase",
                tr("私钥口令"),
                &["passphrase"],
                NetFieldKind::Password,
                false,
                tr("（可选）随私钥使用"),
            ),
            net_spec(
                "password",
                tr("SSH 密码"),
                &["password"],
                NetFieldKind::Password,
                false,
                tr("与私钥二选一"),
            ),
        ],
        "proxy_pwd" | "proxy" => vec![
            net_spec(
                "username",
                tr("代理用户名"),
                &["username"],
                NetFieldKind::Text,
                true,
                tr("代理账号"),
            ),
            net_spec(
                "password",
                tr("代理密码"),
                &["password"],
                NetFieldKind::Password,
                true,
                tr("代理密码"),
            ),
        ],
        "pg_class" => vec![
            net_spec(
                "cert_path",
                tr("客户端证书"),
                &["certPath"],
                NetFieldKind::Text,
                true,
                "/etc/ssl/client.crt",
            ),
            net_spec(
                "cert_key_path",
                tr("客户端私钥"),
                &["certKeyPath"],
                NetFieldKind::Text,
                false,
                tr("（可选）"),
            ),
        ],
        "kerberos" => vec![
            net_spec(
                "principal",
                "Principal",
                &["principal"],
                NetFieldKind::Text,
                true,
                "user@REALM",
            ),
            net_spec(
                "keytab_path",
                tr("Keytab 路径"),
                &["keytabPath"],
                NetFieldKind::Text,
                false,
                tr("（可选）"),
            ),
        ],
        _ => Vec::new(),
    }
}

/// 认证字段值 → `auth_data` JSON（校验必填 / SSH 凭据二选一 / 代理凭据成对）。
pub(crate) fn build_auth_config_json(
    auth_type: &str,
    values: &[(String, String)],
) -> Result<String, String> {
    let key = auth_type.trim().to_ascii_lowercase();
    let specs = auth_field_specs(&key);
    if specs.is_empty() {
        return Err(format!("认证类型「{}」不支持字段编辑", auth_type.trim()));
    }
    let get = |k: &str| -> String {
        values
            .iter()
            .find(|(kk, _)| kk == k)
            .map(|(_, v)| v.trim().to_string())
            .unwrap_or_default()
    };

    for spec in &specs {
        if spec.required && get(spec.key).is_empty() {
            return Err(format!("「{}」为必填项", spec.label));
        }
    }
    let is_ssh = matches!(key.as_str(), "ssh_key" | "ssh");
    let is_proxy = matches!(key.as_str(), "proxy_pwd" | "proxy");
    if is_ssh && get("key_path").is_empty() && get("password").is_empty() {
        return Err("SSH 认证需要「私钥路径」或「密码」二者之一".into());
    }
    if is_proxy && (get("username").is_empty() || get("password").is_empty()) {
        return Err("代理认证需要同时填写用户名与密码".into());
    }

    let mut root = serde_json::Map::new();
    for spec in &specs {
        let raw = get(spec.key);
        if raw.is_empty() {
            continue;
        }
        // 口令只在私钥路径存在时有意义（注入函数只在 keyPath 分支读它）。
        if is_ssh && spec.key == "passphrase" && get("key_path").is_empty() {
            continue;
        }
        insert_json_path(&mut root, spec.path, serde_json::Value::String(raw));
    }
    Ok(serde_json::Value::Object(root).to_string())
}

/// `auth_data` JSON → 字段值（编辑回填；缺失 / 非法 JSON → 全空，不报错）。
pub(crate) fn auth_config_values(auth_type: &str, config_json: &str) -> Vec<(String, String)> {
    spec_field_values(auth_field_specs(auth_type), config_json)
}
