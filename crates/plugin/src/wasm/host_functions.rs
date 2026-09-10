use extism::{CurrentPlugin, Function, UserData, Val, ValType};

pub struct HostFunctionRegistry;

impl Default for HostFunctionRegistry {
    fn default() -> Self {
        Self
    }
}

impl HostFunctionRegistry {
    pub fn new() -> Self {
        Self
    }

    pub fn register_all(plugin_id: &str) -> Vec<Function> {
        vec![
            Self::create_db_query_fn(plugin_id),
            Self::create_db_metadata_fn(plugin_id),
            Self::create_duckdb_query_fn(plugin_id),
            Self::create_duckdb_load_fn(plugin_id),
        ]
    }

    fn create_db_query_fn(_plugin_id: &str) -> Function {
        Function::new(
            "plugin_db_query",
            [ValType::I64, ValType::I64, ValType::I64],
            [ValType::I64],
            UserData::<()>::default(),
            move |_plugin: &mut CurrentPlugin,
                  _inputs: &[Val],
                  _outputs: &mut [Val],
                  _user_data: UserData<()>| {
                Err(extism::Error::msg(
                    "host_db_query requires memory access, use host call instead",
                ))
            },
        )
    }

    fn create_db_metadata_fn(_plugin_id: &str) -> Function {
        Function::new(
            "plugin_db_metadata",
            [
                ValType::I64,
                ValType::I64,
                ValType::I64,
                ValType::I64,
                ValType::I64,
            ],
            [ValType::I64],
            UserData::<()>::default(),
            move |_plugin: &mut CurrentPlugin,
                  _inputs: &[Val],
                  _outputs: &mut [Val],
                  _user_data: UserData<()>| {
                Err(extism::Error::msg(
                    "host_db_metadata requires memory access",
                ))
            },
        )
    }

    fn create_duckdb_query_fn(_plugin_id: &str) -> Function {
        Function::new(
            "plugin_duckdb_query",
            [ValType::I64, ValType::I64],
            [ValType::I64],
            UserData::<()>::default(),
            move |_plugin: &mut CurrentPlugin,
                  _inputs: &[Val],
                  _outputs: &mut [Val],
                  _user_data: UserData<()>| {
                Err(extism::Error::msg(
                    "host_duckdb_query requires memory access",
                ))
            },
        )
    }

    fn create_duckdb_load_fn(_plugin_id: &str) -> Function {
        Function::new(
            "plugin_duckdb_load",
            [ValType::I64, ValType::I64, ValType::I64],
            [ValType::I64],
            UserData::<()>::default(),
            move |_plugin: &mut CurrentPlugin,
                  _inputs: &[Val],
                  _outputs: &mut [Val],
                  _user_data: UserData<()>| {
                Err(extism::Error::msg(
                    "host_duckdb_load requires memory access",
                ))
            },
        )
    }
}
