package rds.jdbc;

import java.sql.DatabaseMetaData;
import java.sql.ResultSet;
import java.util.HashSet;
import java.util.Set;

/**
 * meta.* 五件套（对齐宿主 `sidecar/meta.rs` 的 wire 形状）：
 * catalogs → {"catalogs": [str]}
 * schemas → {"schemas": [{name, comment?}]}
 * objects → {"objects": [{name, kind, comment?, parent?}]}
 * object_detail → {"object": {...}, "columns": [...], "indexes"?, "row_count"?}
 * routine_source → string?（JDBC 无标准例程源码接口，一律 null）
 */
public final class Meta {

    private Meta() {}

    public static Json.Obj catalogs(Session s) {
        Json.Arr out = new Json.Arr();
        try (ResultSet rs = s.meta().getCatalogs()) {
            while (rs.next()) {
                String name = rs.getString(1);
                if (name != null && !name.isBlank()) out.items.add(new Json.Str(name));
            }
        } catch (Exception e) {
            throw new RpcException(RpcException.CODE_SQL_ERROR, "meta.catalogs 失败：" + e.getMessage(), e);
        }
        Json.Obj res = new Json.Obj();
        res.fields.put("catalogs", out);
        return res;
    }

    public static Json.Obj schemas(Session s, String catalog) {
        Json.Arr out = new Json.Arr();
        try (ResultSet rs = s.meta().getSchemas()) {
            while (rs.next()) {
                String name = rs.getString("TABLE_SCHEM");
                String cat = safe(rs, "TABLE_CATALOG");
                if (catalog != null && !catalog.isBlank() && !catalog.equals(cat)) continue;
                if (name == null || name.isBlank()) continue;
                Json.Obj item = new Json.Obj();
                item.fields.put("name", new Json.Str(name));
                out.items.add(item);
            }
        } catch (Exception e) {
            throw new RpcException(RpcException.CODE_SQL_ERROR, "meta.schemas 失败：" + e.getMessage(), e);
        }
        Json.Obj res = new Json.Obj();
        res.fields.put("schemas", out);
        return res;
    }

    public static Json.Obj objects(Session s, String catalog, String schema) {
        Json.Arr out = new Json.Arr();
        try {
            // 一次给全（宿主硬口径）：表 + 视图 + 例程 + 序列
            collectObjects(s, catalog, schema, out, false);
            collectObjects(s, catalog, schema, out, true);
        } catch (Exception e) {
            throw new RpcException(RpcException.CODE_SQL_ERROR, "meta.objects 失败：" + e.getMessage(), e);
        }
        Json.Obj res = new Json.Obj();
        res.fields.put("objects", out);
        return res;
    }

    private static void collectObjects(Session s, String catalog, String schema,
                                       Json.Arr out, boolean routines) throws Exception {
        if (!routines) {
            try (ResultSet rs = s.meta().getTables(catalog, schema, "%", null)) {
                while (rs.next()) {
                    String name = rs.getString("TABLE_NAME");
                    if (name == null) continue;
                    String type = safe(rs, "TABLE_TYPE");
                    String kind = switch (type == null ? "" : type.toUpperCase()) {
                        case "VIEW" -> "view";
                        case "SYSTEM TABLE", "SYSTEM VIEW", "GLOBAL TEMPORARY", "LOCAL TEMPORARY" -> type.toLowerCase();
                        default -> "table";
                    };
                    out.items.add(objectItem(name, kind, null, schema));
                }
            }
        } else {
            try (ResultSet rs = s.meta().getProcedures(catalog, schema, "%")) {
                while (rs.next()) {
                    String name = rs.getString("PROCEDURE_NAME");
                    if (name == null) continue;
                    out.items.add(objectItem(name, "procedure", null, schema));
                }
            } catch (Exception ignored) {
                // 有的驱动不支持 getProcedures（如 H2 老版）——少一类对象不是错误
            }
            try (ResultSet rs = s.meta().getFunctions(catalog, schema, "%")) {
                while (rs.next()) {
                    String name = rs.getString("FUNCTION_NAME");
                    if (name == null) continue;
                    out.items.add(objectItem(name, "function", null, schema));
                }
            } catch (Exception ignored) {
            }
        }
    }

    public static Json.Obj objectDetail(Session s, String catalog, String schema, String object) {
        Json.Obj objectNode = new Json.Obj();
        objectNode.fields.put("name", new Json.Str(object));
        objectNode.fields.put("kind", new Json.Str("table"));
        objectNode.fields.put("parent", new Json.Str(schema == null ? "" : schema));

        Json.Arr columns = new Json.Arr();
        Set<String> pks = new HashSet<>();
        Set<String> fks = new HashSet<>();
        try {
            try (ResultSet rs = s.meta().getPrimaryKeys(catalog, schema, object)) {
                while (rs.next()) pks.add(safe(rs, "COLUMN_NAME"));
            }
            try (ResultSet rs = s.meta().getImportedKeys(catalog, schema, object)) {
                while (rs.next()) fks.add(safe(rs, "FKCOLUMN_NAME"));
            }
            try (ResultSet rs = s.meta().getColumns(catalog, schema, object, "%")) {
                while (rs.next()) {
                    String name = rs.getString("COLUMN_NAME");
                    if (name == null) continue;
                    Json.Obj col = new Json.Obj();
                    col.fields.put("name", new Json.Str(name));
                    col.fields.put("type_raw", new Json.Str(safe(rs, "TYPE_NAME")));
                    int nullable = rs.getInt("NULLABLE");
                    col.fields.put("nullable", new Json.Bool(nullable != java.sql.DatabaseMetaData.columnNoNulls));
                    col.fields.put("is_pk", new Json.Bool(pks.contains(name)));
                    col.fields.put("is_fk", new Json.Bool(fks.contains(name)));
                    String def = safe(rs, "COLUMN_DEF");
                    if (def != null && !def.isBlank()) col.fields.put("default", new Json.Str(def));
                    String remark = safe(rs, "REMARKS");
                    if (remark != null && !remark.isBlank()) col.fields.put("comment", new Json.Str(remark));
                    columns.items.add(col);
                }
            }
        } catch (Exception e) {
            throw new RpcException(RpcException.CODE_SQL_ERROR, "meta.object_detail 失败：" + e.getMessage(), e);
        }

        Json.Obj res = new Json.Obj();
        res.fields.put("object", objectNode);
        res.fields.put("columns", columns);
        return res;
    }

    public static Json.Value routineSource() {
        return Json.Null.INSTANCE; // JDBC 无标准例程源码接口
    }

    private static Json.Obj objectItem(String name, String kind, String comment, String parent) {
        Json.Obj item = new Json.Obj();
        item.fields.put("name", new Json.Str(name));
        item.fields.put("kind", new Json.Str(kind));
        if (comment != null) item.fields.put("comment", new Json.Str(comment));
        if (parent != null) item.fields.put("parent", new Json.Str(parent));
        return item;
    }

    /** 列名取不到/为空时给空串（不少驱动缺列）。 */
    private static String safe(ResultSet rs, String column) {
        try {
            String v = rs.getString(column);
            return v == null ? "" : v;
        } catch (Exception e) {
            return "";
        }
    }
}
