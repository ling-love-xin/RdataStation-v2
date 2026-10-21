package rds.jdbc;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * 极简 JSON（协议够用即可）——通用壳不引外部依赖，解析与序列化各一份。
 *
 * 支持的类型：null / boolean / number（long + double）/ string（含反斜杠加 u 的
 * 四位十六进制转义）/ 数组 / 对象。协议里出现的字段与结果值都在这几类里。
 *
 * 约定：{@link #parse(String)} 的返回都是本包内的 {@link Value}；数字统一先按
 * long 试，带小数点/指数才按 double。
 */
public final class Json {

    /** JSON 值。 */
    public abstract static class Value {
        public boolean isNull() { return this instanceof Null; }
        public boolean isBool() { return this instanceof Bool; }
        public boolean isNumber() { return this instanceof Num; }
        public boolean isString() { return this instanceof Str; }
        public boolean isArray() { return this instanceof Arr; }
        public boolean isObject() { return this instanceof Obj; }

        public String asString() { throw new IllegalStateException("不是字符串"); }
        public long asLong() { throw new IllegalStateException("不是数字"); }
        public double asDouble() { throw new IllegalStateException("不是数字"); }
        public boolean asBool() { throw new IllegalStateException("不是布尔"); }
        public List<Value> asArray() { throw new IllegalStateException("不是数组"); }
        public Map<String, Value> asObject() { throw new IllegalStateException("不是对象"); }

        /** 序列化为紧凑 JSON 文本。 */
        public abstract void write(StringBuilder sb);

        @Override
        public String toString() {
            StringBuilder sb = new StringBuilder();
            write(sb);
            return sb.toString();
        }
    }

    public static final class Null extends Value {
        public static final Null INSTANCE = new Null();
        @Override public void write(StringBuilder sb) { sb.append("null"); }
    }

    public static final class Bool extends Value {
        public final boolean value;
        public Bool(boolean value) { this.value = value; }
        @Override public boolean asBool() { return value; }
        @Override public void write(StringBuilder sb) { sb.append(value ? "true" : "false"); }
    }

    public static final class Num extends Value {
        public final long longValue;
        public final double doubleValue;
        public final boolean isIntegral;
        public Num(long v) { this.longValue = v; this.doubleValue = v; this.isIntegral = true; }
        public Num(double v) { this.longValue = (long) v; this.doubleValue = v; this.isIntegral = false; }
        @Override public long asLong() { return longValue; }
        @Override public double asDouble() { return doubleValue; }
        @Override public void write(StringBuilder sb) {
            if (isIntegral) sb.append(longValue);
            else if (doubleValue == Math.floor(doubleValue) && !Double.isInfinite(doubleValue)) {
                // 整数型 double 不带小数点输出（避免 1.0 这种）
                sb.append((long) doubleValue);
            } else {
                sb.append(doubleValue);
            }
        }
    }

    public static final class Str extends Value {
        public final String value;
        public Str(String value) { this.value = value; }
        @Override public String asString() { return value; }
        @Override public void write(StringBuilder sb) {
            sb.append('"');
            for (int i = 0; i < value.length(); i++) {
                char c = value.charAt(i);
                switch (c) {
                    case '"': sb.append("\\\""); break;
                    case '\\': sb.append("\\\\"); break;
                    case '\n': sb.append("\\n"); break;
                    case '\r': sb.append("\\r"); break;
                    case '\t': sb.append("\\t"); break;
                    case '\b': sb.append("\\b"); break;
                    case '\f': sb.append("\\f"); break;
                    default:
                        if (c < 0x20) sb.append(String.format("\\u%04x", (int) c));
                        else sb.append(c);
                }
            }
            sb.append('"');
        }
    }

    public static final class Arr extends Value {
        public final List<Value> items = new ArrayList<>();
        @Override public List<Value> asArray() { return items; }
        @Override public void write(StringBuilder sb) {
            sb.append('[');
            for (int i = 0; i < items.size(); i++) {
                if (i > 0) sb.append(',');
                items.get(i).write(sb);
            }
            sb.append(']');
        }
    }

    public static final class Obj extends Value {
        public final Map<String, Value> fields = new LinkedHashMap<>();
        @Override public Map<String, Value> asObject() { return fields; }
        @Override public void write(StringBuilder sb) {
            sb.append('{');
            boolean first = true;
            for (Map.Entry<String, Value> e : fields.entrySet()) {
                if (!first) sb.append(',');
                first = false;
                new Str(e.getKey()).write(sb);
                sb.append(':');
                e.getValue().write(sb);
            }
            sb.append('}');
        }
    }

    private Json() {}

    // ==================== 解析 ====================

    /** 解析 JSON 文本（根值必须完整，尾随空白/多余内容报错）。 */
    public static Value parse(String text) {
        Parser p = new Parser(text);
        Value v = p.parseValue();
        p.skipWs();
        if (!p.atEnd()) throw new IllegalArgumentException("JSON 尾部有多余内容");
        return v;
    }

    /** 按路径取对象字段（a.b.c）；取不到返回 null。 */
    public static Value path(Value root, String... keys) {
        Value cur = root;
        for (String k : keys) {
            if (!cur.isObject()) return null;
            cur = cur.asObject().get(k);
            if (cur == null) return null;
        }
        return cur;
    }

    private static final class Parser {
        private final String s;
        private int i;

        Parser(String s) { this.s = s; }

        boolean atEnd() { return i >= s.length(); }
        char peek() { return s.charAt(i); }
        char next() { return s.charAt(i++); }

        void skipWs() {
            while (i < s.length()) {
                char c = s.charAt(i);
                if (c == ' ' || c == '\t' || c == '\n' || c == '\r') i++;
                else break;
            }
        }

        Value parseValue() {
            skipWs();
            if (atEnd()) throw err("意外结尾");
            char c = peek();
            switch (c) {
                case '{': return parseObject();
                case '[': return parseArray();
                case '"': return new Str(parseString());
                case 't': expect("true"); return new Bool(true);
                case 'f': expect("false"); return new Bool(false);
                case 'n': expect("null"); return Null.INSTANCE;
                default: return parseNumber();
            }
        }

        Obj parseObject() {
            next(); // {
            Obj obj = new Obj();
            skipWs();
            if (peek() == '}') { next(); return obj; }
            while (true) {
                skipWs();
                if (peek() != '"') throw err("对象键必须是字符串");
                String key = parseString();
                skipWs();
                if (next() != ':') throw err("对象缺冒号");
                Value v = parseValue();
                obj.fields.put(key, v);
                skipWs();
                char c = next();
                if (c == ',') continue;
                if (c == '}') return obj;
                throw err("对象缺逗号/右花括号");
            }
        }

        Arr parseArray() {
            next(); // [
            Arr arr = new Arr();
            skipWs();
            if (peek() == ']') { next(); return arr; }
            while (true) {
                arr.items.add(parseValue());
                skipWs();
                char c = next();
                if (c == ',') continue;
                if (c == ']') return arr;
                throw err("数组缺逗号/右方括号");
            }
        }

        String parseString() {
            next(); // "
            StringBuilder sb = new StringBuilder();
            while (true) {
                if (atEnd()) throw err("字符串未闭合");
                char c = next();
                if (c == '"') return sb.toString();
                if (c == '\\') {
                    if (atEnd()) throw err("转义未闭合");
                    char e = next();
                    switch (e) {
                        case '"': sb.append('"'); break;
                        case '\\': sb.append('\\'); break;
                        case '/': sb.append('/'); break;
                        case 'n': sb.append('\n'); break;
                        case 'r': sb.append('\r'); break;
                        case 't': sb.append('\t'); break;
                        case 'b': sb.append('\b'); break;
                        case 'f': sb.append('\f'); break;
                        case 'u': {
                            if (i + 4 > s.length()) throw err("\\u 转义不完整");
                            int code = Integer.parseInt(s.substring(i, i + 4), 16);
                            i += 4;
                            sb.append((char) code);
                            break;
                        }
                        default: throw err("未知转义 \\" + e);
                    }
                } else {
                    sb.append(c);
                }
            }
        }

        Value parseNumber() {
            int start = i;
            if (peek() == '-') next();
            while (i < s.length() && Character.isDigit(s.charAt(i))) i++;
            boolean isDouble = false;
            if (i < s.length() && s.charAt(i) == '.') { isDouble = true; next(); while (i < s.length() && Character.isDigit(s.charAt(i))) i++; }
            if (i < s.length() && (s.charAt(i) == 'e' || s.charAt(i) == 'E')) {
                isDouble = true;
                next();
                if (i < s.length() && (s.charAt(i) == '+' || s.charAt(i) == '-')) next();
                while (i < s.length() && Character.isDigit(s.charAt(i))) i++;
            }
            String text = s.substring(start, i);
            try {
                if (isDouble) return new Num(Double.parseDouble(text));
                return new Num(Long.parseLong(text));
            } catch (NumberFormatException ex) {
                throw err("数字格式错误：" + text);
            }
        }

        void expect(String word) {
            if (!s.startsWith(word, i)) throw err("期望 " + word);
            i += word.length();
        }

        IllegalArgumentException err(String msg) {
            return new IllegalArgumentException("JSON 解析错误@" + i + "：" + msg);
        }
    }
}
