//! 容错解析（设计文档 §5.3）：JSON 修复 → markdown 提取 → 正则回退。
//!
//! 小模型最常见的失败是"工具调用格式不对"：多一层解释文字、尾逗号、单引号、
//! 参数被序列化成字符串。这里的顺序是硬性的：能修就修，修不了才回退。

use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub name: String,
    pub arguments: Value,
}

/// 从已解析的结构化值（如 provider 的 `tool_calls` 数组）提取工具调用。
/// 与文本解析共用同一套归一化逻辑，兼容 Ollama 的 arguments 对象和 OpenAI 的 JSON 字符串。
pub fn from_value(v: &Value) -> Vec<ToolCall> {
    let mut out = Vec::new();
    collect_from_value(v, &mut out);
    dedup(out)
}

/// 从模型输出中提取工具调用；解析不出就返回空（绝不抛错）。
pub fn extract(text: &str) -> Vec<ToolCall> {
    let mut out = Vec::new();

    // 1) 直接整体解析
    if let Ok(v) = serde_json::from_str::<Value>(text.trim()) {
        collect_from_value(&v, &mut out);
    }

    // 2) markdown 围栏
    for block in fenced_blocks(text) {
        for candidate in [block.as_str(), &repair(&block)] {
            if let Ok(v) = serde_json::from_str::<Value>(candidate) {
                collect_from_value(&v, &mut out);
            }
        }
    }

    // 3) 扫描平衡的 `{...}` 片段（先原文，再修复）
    for frag in balanced_fragments(text) {
        for candidate in [frag.as_str(), &repair(&frag)] {
            if let Ok(v) = serde_json::from_str::<Value>(candidate) {
                collect_from_value(&v, &mut out);
            }
        }
    }

    // 4) 纯文本工具调用：小模型常把调用写成正文
    // `tool_call tool.fs.write {"path":"..."}`，没有 name/arguments 包装。
    if out.is_empty() {
        if let Some(c) = plain_tool_line(text) {
            out.push(c);
        }
    }

    // 5) 正则回退：`name` + `arguments` 手工抽取
    if out.is_empty() {
        if let Some(c) = regex_fallback(text) {
            out.push(c);
        }
    }

    // 6) `<tool_call>` XML 伪标签
    if out.is_empty() {
        for frag in tag_fragments(text, "tool_call") {
            if let Some(c) = regex_fallback(&frag) {
                out.push(c);
            }
        }
    }

    // 7) Qwen/Ollama 的 `<function=...><parameter=...>` 变体；小模型在
    // review feedback 后偶发输出这种 XML，Ollama 自身也可能报 XML syntax error。
    if out.is_empty() {
        if let Some(c) = xml_tool_call(text) {
            out.push(c);
        }
    }

    dedup(out)
}

fn collect_from_value(v: &Value, out: &mut Vec<ToolCall>) {
    match v {
        Value::Array(arr) => {
            for x in arr {
                collect_from_value(x, out);
            }
        }
        Value::Object(map) => {
            // OpenAI 风格：{"name": "...", "arguments": {...}} 或 {"function": {...}}
            let name = map
                .get("name")
                .or_else(|| map.get("tool"))
                .or_else(|| map.get("tool_name"))
                .and_then(|n| n.as_str());
            if let Some(name) = name {
                let args = map
                    .get("arguments")
                    .or_else(|| map.get("parameters"))
                    .or_else(|| map.get("args"))
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                match args {
                    Value::String(s) => {
                        let parsed =
                            serde_json::from_str(&s).unwrap_or_else(|_| json!({ "raw": s }));
                        out.push(ToolCall {
                            name: name.to_string(),
                            arguments: parsed,
                        });
                    }
                    other => out.push(ToolCall {
                        name: name.to_string(),
                        arguments: other,
                    }),
                }
            }
            if let Some(func) = map.get("function") {
                collect_from_value(func, out);
            }
            // tool_calls 数组
            if let Some(Value::Array(calls)) = map.get("tool_calls") {
                for c in calls {
                    collect_from_value(c, out);
                }
            }
        }
        _ => {}
    }
}

fn fenced_blocks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("```") {
        let after = &rest[start + 3..];
        let body_start = after.find('\n').map(|i| i + 1).unwrap_or(0);
        let body = &after[body_start..];
        if let Some(end) = body.find("```") {
            let mut s = body[..end].trim();
            // 去掉语言标记行
            if let Some(first_nl) = s.find('\n') {
                let first = &s[..first_nl];
                if first.starts_with("json")
                    || first.starts_with("JSON")
                    || first.starts_with("tool")
                {
                    s = s[first_nl + 1..].trim();
                }
            }
            out.push(s.to_string());
            rest = &body[end + 3..];
        } else {
            break;
        }
    }
    out
}

fn tag_fragments(text: &str, tag: &str) -> Vec<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(s) = rest.find(&open) {
        let after = &rest[s + open.len()..];
        if let Some(e) = after.find(&close) {
            out.push(after[..e].trim().to_string());
            rest = &after[e + close.len()..];
        } else {
            out.push(after.trim().to_string());
            break;
        }
    }
    out
}

/// 取出所有顶层平衡的 `{...}` 片段（忽略字符串内的括号）。
fn balanced_fragments(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(end) = balanced_end(text, i) {
                out.push(text[i..end].to_string());
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn balanced_end(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    for (i, b) in bytes.iter().enumerate().skip(start) {
        let b = *b;
        if in_str {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// JSON 修复：尾逗号、单引号、裸键、中文引号。
pub fn repair(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    let mut chars = s.chars().peekable();
    let mut in_str = false;
    let mut quote_char = '"';
    while let Some(c) = chars.next() {
        match c {
            '“' | '”' if !in_str => {
                quote_char = '"';
                in_str = true;
                out.push('"');
            }
            '‘' | '’' if !in_str => {
                quote_char = '\'';
                in_str = true;
                out.push('"');
            }
            '\'' if !in_str => {
                quote_char = '\'';
                in_str = true;
                out.push('"');
            }
            '"' if !in_str => {
                quote_char = '"';
                in_str = true;
                out.push('"');
            }
            c if in_str && c == quote_char => {
                in_str = false;
                out.push('"');
            }
            '\\' if in_str => {
                out.push('\\');
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            }
            ',' if !in_str => {
                // 尾逗号：后面跳过空白后若是 } 或 ] 则不写
                let look = chars.clone();
                let mut next = None;
                for x in look {
                    if !x.is_whitespace() {
                        next = Some(x);
                        break;
                    }
                }
                if matches!(next, Some('}') | Some(']')) {
                    continue;
                }
                out.push(',');
            }
            _ => out.push(c),
        }
    }
    quote_bare_keys(&out)
}

fn quote_bare_keys(s: &str) -> String {
    // { name: ... } → { "name": ... }
    // 注意：必须按字节操作但**保留原始 UTF-8 字节**，不能把每个字节当 char push（中文会变乱码）。
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len() + 8);
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'{' || bytes[i] == b',' {
            out.push(bytes[i]);
            i += 1;
            // 跳过空白，找裸键
            let mut j = i;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                out.push(bytes[j]);
                j += 1;
            }
            let start = j;
            while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                j += 1;
            }
            if j > start && j < bytes.len() && bytes[j] == b':' {
                out.push(b'"');
                out.extend_from_slice(&bytes[start..j]);
                out.push(b'"');
                out.push(b':');
                i = j + 1;
                continue;
            }
            i = j;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// N15：解析小模型偶发的函数式伪标签：`<function=tool.fs.write>` 后跟若干
/// `<parameter=path>…` 参数；也兼容 `name="..."` 写法与缺失结束标签的畸形输出。
fn xml_tool_call(text: &str) -> Option<ToolCall> {
    const PARAM_CLOSE: &str = concat!("</", "parameter>");
    let start = text.find("<function")?;
    let tag_end = start + text[start..].find('>')?;
    let tag = &text[start..=tag_end];
    let name = xml_attr(tag, "function")?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let body = &text[tag_end + 1..];
    let mut args = serde_json::Map::new();
    let mut rest = body;
    while let Some(pos) = rest.find("<parameter") {
        let part = &rest[pos..];
        let open_end = part.find('>')?;
        let open = &part[..=open_end];
        let key = xml_attr(open, "parameter")?;
        let after = &part[open_end + 1..];
        let close = after.find(PARAM_CLOSE);
        let next_param = after.find("<parameter");
        let stop = match (close, next_param) {
            (Some(c), Some(p)) => c.min(p),
            (Some(c), None) => c,
            (None, Some(p)) => p,
            (None, None) => after.len(),
        };
        let raw = &after[..stop];
        let next = if close == Some(stop) {
            after[stop..]
                .find('>')
                .map(|j| stop + j + 1)
                .unwrap_or(after.len())
        } else {
            stop
        };
        args.insert(key, Value::String(raw.trim().to_string()));
        rest = &after[next..];
    }
    if args.is_empty() {
        let trimmed = body.trim();
        if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(trimmed) {
            args = map;
        } else {
            return None;
        }
    }
    let value = Value::Object(args);
    Some(ToolCall {
        name: alias_name(name, &value),
        arguments: value,
    })
}

/// 读取伪标签里的 `key=value` 或 `key name="value"` 属性值。
fn xml_attr(tag: &str, key: &str) -> Option<String> {
    let inner = tag
        .trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim();
    let rest = inner.strip_prefix(key)?.trim_start();
    let rest = if let Some(r) = rest.strip_prefix('=') {
        r
    } else {
        let r = rest.strip_prefix("name")?.trim_start();
        r.strip_prefix('=')?
    };
    let value = rest.trim_start();
    let value = value.trim_start_matches(['"', '\'']);
    let end = value
        .find(|c: char| c == '"' || c == '\'' || c == '>' || c.is_whitespace())
        .unwrap_or(value.len());
    if end == 0 {
        None
    } else {
        Some(value[..end].to_string())
    }
}

/// N11：解析 `tool_call <name> <json>` / `tool_call: <name> <json>` 这类纯文本调用；
/// 它没有 `"name"`/`"arguments"` 包装，正则回退看不到，容易被当成最终答案。
fn plain_tool_line(text: &str) -> Option<ToolCall> {
    let mut rest = text;
    while let Some(pos) = rest.find("tool_call") {
        let mut after = &rest[pos + "tool_call".len()..];
        after = after.trim_start();
        after = after.strip_prefix(':').unwrap_or(after).trim_start();
        let name_end = after.find(char::is_whitespace)?;
        let name = after[..name_end].trim();
        let args = after[name_end..].trim_start();
        if name.is_empty() || !args.starts_with('{') {
            rest = after;
            continue;
        }
        let end = balanced_end(args, 0)?;
        let raw = &args[..end];
        let value = serde_json::from_str::<Value>(raw)
            .or_else(|_| serde_json::from_str::<Value>(&repair(raw)))
            .ok();
        if let Some(value) = value {
            return Some(ToolCall {
                name: alias_name(name, &value),
                arguments: value,
            });
        }
        rest = after;
    }
    None
}

fn alias_name(name: &str, args: &Value) -> String {
    match name.trim() {
        "tool_fs_read" => "tool.fs.read".into(),
        "tool_fs_write" => "tool.fs.write".into(),
        "tool_fs_list" => "tool.fs.list".into(),
        "tool_shell" => "tool.shell".into(),
        // 保留裸 `tool.fs` 作为未知工具，交给既有纠错提示；只有带 content 时才明确是写。
        "tool.fs" | "tool_fs" if args.get("content").is_some() => "tool.fs.write".into(),
        "tool_fs" => "tool.fs.read".into(),
        other => other.to_string(),
    }
}

fn regex_fallback(text: &str) -> Option<ToolCall> {
    let name = find_string_after(text, "\"name\"")?;
    if name.is_empty() {
        return None;
    }
    let args = find_after(text, "\"arguments\"").or_else(|| find_after(text, "\"parameters\""))?;
    let args_trim = args.trim_start();
    let value = if args_trim.starts_with('{') {
        // 参数对象必须闭合；半截 JSON 直接放弃（宁可漏一次调用，也不发错参数）
        let end = balanced_end(args_trim, 0)?;
        let raw = &args_trim[..end];
        serde_json::from_str::<Value>(raw)
            .or_else(|_| serde_json::from_str::<Value>(&repair(raw)))
            .ok()?
    } else if args_trim.starts_with('"') {
        let s = find_string_after(text, "\"arguments\"").unwrap_or_default();
        serde_json::from_str(&s).unwrap_or_else(|_| json!({"raw": s}))
    } else {
        json!({"raw": args_trim})
    };
    Some(ToolCall {
        name,
        arguments: value,
    })
}

fn find_after<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let idx = text.find(key)?;
    let after = &text[idx + key.len()..];
    let colon = after.find(':')?;
    Some(&after[colon + 1..])
}

fn find_string_after(text: &str, key: &str) -> Option<String> {
    let rest = find_after(text, key)?;
    let rest = rest.trim_start();
    let quote = rest.find('"')?;
    let body = &rest[quote + 1..];
    let end = body.find('"')?;
    Some(body[..end].to_string())
}

fn dedup(calls: Vec<ToolCall>) -> Vec<ToolCall> {
    let mut out: Vec<ToolCall> = Vec::new();
    for c in calls {
        let name = alias_name(&c.name, &c.arguments);
        if !out
            .iter()
            .any(|x| x.name == name && x.arguments == c.arguments)
        {
            out.push(ToolCall {
                name,
                arguments: c.arguments,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_ollama_tool_calls() {
        let v =
            json!([{"function":{"name":"tool.fs.read","arguments":{"path":"workspace/a.txt"}}}]);
        let c = from_value(&v);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].name, "tool.fs.read");
        assert_eq!(c[0].arguments["path"], "workspace/a.txt");
    }

    #[test]
    fn structured_openai_tool_calls_string_arguments() {
        let v = json!([{"id":"call_1","function":{"name":"tool.fs.read","arguments":"{\"path\":\"a.txt\"}"}}]);
        let c = from_value(&v);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].arguments["path"], "a.txt");
    }

    #[test]
    fn plain_json_and_openai_shape() {
        let text = r#"{"name":"tool.fs.read","arguments":{"path":"/tmp/a"}}"#;
        let c = extract(text);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].name, "tool.fs.read");
        assert_eq!(c[0].arguments["path"], "/tmp/a");
    }

    #[test]
    fn fenced_block_with_chatter() {
        let text = "我来读取文件。\n```json\n{\"name\":\"tool.fs.read\",\"arguments\":{\"path\":\"/etc/hosts\"}}\n```\n完成。";
        let c = extract(text);
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn repair_trailing_comma_and_single_quotes() {
        let text = "```\n{ 'name': 'tool.shell', 'arguments': {'cmd': 'ls',}, }\n```";
        let c = extract(text);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].name, "tool.shell");
        assert_eq!(c[0].arguments["cmd"], "ls");
    }

    #[test]
    fn args_as_json_string() {
        let text = r#"{"name":"tool.fs.write","arguments":"{\"path\":\"a\",\"content\":\"b\"}"}"#;
        let c = extract(text);
        assert_eq!(c[0].arguments["path"], "a");
    }

    #[test]
    fn xml_tag_fallback() {
        let text =
            "<tool_call>{\"name\": \"tool.fs.read\", \"arguments\": {\"path\": \"x\"}}</tool_call>";
        let c = extract(text);
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn garbage_returns_empty() {
        assert!(extract("我今天不想调用工具，只想聊聊。").is_empty());
        assert!(extract("{\"name\": \"tool.fs.read\", \"arguments\": {").is_empty());
    }

    #[test]
    fn plain_text_tool_call_line_is_extracted() {
        let text = "tool_call tool.fs.write {\"path\":\"workspace/notes.txt\",\"content\":\"- a\"}";
        let c = extract(text);
        assert_eq!(c.len(), 1, "{c:?}");
        assert_eq!(c[0].name, "tool.fs.write");
        assert_eq!(c[0].arguments["path"], "workspace/notes.txt");
    }

    #[test]
    fn plain_text_tool_call_aliases_are_normalized() {
        let text = "调用：\ntool_call: tool_fs_read {\"path\":\"workspace/notes.txt\"}";
        let c = extract(text);
        assert_eq!(c.len(), 1, "{c:?}");
        assert_eq!(c[0].name, "tool.fs.read");
        assert_eq!(c[0].arguments["path"], "workspace/notes.txt");
    }

    #[test]
    fn xml_function_parameter_fallback() {
        let pclose = concat!("</", "parameter>");
        let fclose = concat!("</", "function>");
        let text = format!(
            "<function=tool.fs.write><parameter=path>workspace/notes.txt{pclose}<parameter=content>- a\n- b{pclose}{fclose}"
        );
        let c = extract(&text);
        assert_eq!(c.len(), 1, "{c:?}");
        assert_eq!(c[0].name, "tool.fs.write");
        assert_eq!(c[0].arguments["path"], "workspace/notes.txt");
        assert_eq!(c[0].arguments["content"], "- a\n- b");
    }

    #[test]
    fn malformed_xml_function_without_close_is_recovered() {
        let pclose = concat!("</", "parameter>");
        let text = format!(
            "<function=tool.fs.write><parameter=path>workspace/notes.txt{pclose}<parameter=content>done{pclose}"
        );
        let c = extract(&text);
        assert_eq!(c.len(), 1, "{c:?}");
        assert_eq!(c[0].name, "tool.fs.write");
        assert_eq!(c[0].arguments["path"], "workspace/notes.txt");
        assert_eq!(c[0].arguments["content"], "done");
    }

    #[test]
    fn nested_objects_balance() {
        let text =
            r#"{"name":"tool.shell","arguments":{"cmd":"echo {a:1}","env":{"A":"b"},"n":3}}"#;
        let c = extract(text);
        assert_eq!(c[0].arguments["env"]["A"], "b");
        assert_eq!(c[0].arguments["n"], 3);
    }
}
