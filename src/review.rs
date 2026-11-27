//! Review-Gate 的判据与审批解析层。
//!
//! 从 `src/bin/plugin.rs` 抽出：这里只处理“模型文本 → 结构化判据/审批”的纯函数，
//! 不依赖插件运行时、不落盘、不发网络请求，便于单测与复用。
//!
//! N32：本地 7B 模型经常在 JSON 字符串里原样输出 Windows 路径（`C:\Users\...`），
//! 反斜杠被当成非法转义。`parse_json_value` 先按标准 JSON 解析，失败后只修复
//! 字符串内的非法转义/裸控制字符，再解析一次；不做全局替换，避免破坏合法 JSON。

use serde_json::{json, Value};

fn json_slice(text: &str) -> &str {
    let t = text.trim();
    let t = t.strip_prefix("```json").unwrap_or(t);
    let t = t.strip_prefix("```").unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t).trim();
    let obj = match (t.find('{'), t.rfind('}')) {
        (Some(a), Some(b)) if b > a => Some((a, b)),
        _ => None,
    };
    let arr = match (t.find('['), t.rfind(']')) {
        (Some(a), Some(b)) if b > a => Some((a, b)),
        _ => None,
    };
    match (obj, arr) {
        (Some((oa, ob)), Some((aa, ab))) => {
            if aa < oa {
                return &t[aa..=ab];
            }
            return &t[oa..=ob];
        }
        (Some((a, b)), None) => return &t[a..=b],
        (None, Some((a, b))) => return &t[a..=b],
        _ => {}
    }
    t
}

pub fn sentence_like(s: &str) -> bool {
    let t = s.trim();
    if t.ends_with(['。', '！', '？', '!', '?', '.']) || t.contains(['。', '！', '？']) {
        return true;
    }
    // 没有句末标点但带“是/的/了/和/并/而/为”这类连接词、且够长，也当整句处理：
    // 4B 模型最爱的写法就是“X 是 Y 的 Z”，把这种当关键词等于把答案写进判据。
    t.chars().count() >= 12
        && ["是", "的", "了", "和", "并", "而", "为", "把", "将", "以及"]
            .iter()
            .any(|m| t.contains(m))
}

/// 归一化答案与 evidence：去掉 markdown 标记并压缩空白，再做连续子串比对。
fn quote_norm(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '*' | '`' | '#' | '>' | '_' | '~'))
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("")
}

const EVIDENCE_MIN_CJK: usize = 6;
const EVIDENCE_MIN_WORDS: usize = 3;
const EVIDENCE_QUOTE_CHARS: usize = 6;

/// N36：中文短句 9 字（如“三条要点已写入文件”）不应被英文式的 10 字门限误杀。
/// 判据改为“至少 6 个汉字，或至少 3 个英文词”，仍拒绝“总结/通过/ok”这类占位。
fn evidence_shape(s: &str) -> (usize, usize) {
    let cjk = s
        .chars()
        .filter(|c| matches!(*c as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF))
        .count();
    let words = s
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| {
            let mut chars = w.chars();
            let Some(first) = chars.next() else {
                return false;
            };
            first.is_ascii_alphabetic() && w.chars().count() >= 2
        })
        .count();
    (cjk, words)
}

fn evidence_is_substantive(s: &str) -> bool {
    let (cjk, words) = evidence_shape(s);
    let ascii_alnum = s.chars().filter(|c| c.is_ascii_alphanumeric()).count();
    // 长标识符/路径（如 REVIEW-FINAL）也算有效证据；纯占位词仍然过不了这一关。
    cjk >= EVIDENCE_MIN_CJK || words >= EVIDENCE_MIN_WORDS || ascii_alnum >= 10
}

/// evidence 必须能在答案里找到连续 ≥6 字（答案不足 6 字时按答案长度算）。
fn evidence_quotes(answer: &str, evidence: &str) -> bool {
    let a: Vec<char> = quote_norm(answer).chars().collect();
    let e: Vec<char> = quote_norm(evidence).chars().collect();
    if a.is_empty() {
        return true;
    }
    let need = EVIDENCE_QUOTE_CHARS.min(a.len());
    if e.len() < need {
        return false;
    }
    for i in 0..=(e.len() - need) {
        if a.windows(need).any(|w| w == &e[i..i + need]) {
            return true;
        }
    }
    false
}

fn quoted_literal(check: &str) -> Option<String> {
    for (open, close) in [
        ('『', '』'),
        ('「', '」'),
        ('`', '`'),
        ('“', '”'),
        ('"', '"'),
    ] {
        if let Some(a) = check.find(open) {
            let rest = &check[a + open.len_utf8()..];
            if let Some(b) = rest.find(close) {
                let lit = rest[..b].trim();
                if !lit.is_empty() {
                    return Some(lit.to_string());
                }
            }
        }
    }
    None
}

const RUBRIC_MIN_ITEMS: usize = 1;
const RUBRIC_MAX_ITEMS: usize = 5;
/// 单条字面判据上限：超过就是“把答案写进判据”。
const MUST_INCLUDE_MAX_CHARS: usize = 12;
const PATH_MUST_INCLUDE_MAX_CHARS: usize = 64;

/// N10：路径/文件名（含 `/`、`\` 或带扩展名的点号形式）允许更长；
/// 这不是放宽普通术语，而是把“文件产出”与“答案原句”区分开。
fn path_like(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return false;
    }
    if t.contains('/') || t.contains('\\') {
        return true;
    }
    match t.rsplit_once('.') {
        Some((stem, ext)) => {
            !stem.is_empty()
                && !ext.is_empty()
                && ext.len() <= 8
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
        }
        None => false,
    }
}
const MUST_NOT_INCLUDE_MAX_CHARS: usize = 20;
const PATTERN_MAX_CHARS: usize = 120;
/// 正则判据上限：太长的正则会拖慢 hardcheck，也不好审。
const REGEX_MAX_CHARS: usize = 200;

/// 把 `must_not_include` 的 string/array 两种写法都收敛成列表。
fn literal_list(v: &Value) -> Vec<String> {
    match v {
        Value::String(s) => vec![s.trim().to_string()],
        Value::Array(a) => a
            .iter()
            .filter_map(|x| x.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// 概念性连接词/系词：must_include 是术语判据，不应写自然语言描述。
fn include_connective(t: &str) -> Option<char> {
    t.chars()
        .find(|c| matches!(c, '和' | '与' | '及' | '或' | '是' | '的' | '了'))
}

/// 把 `must_include` 拆成需要同时出现的词项：空白、中文顿号/逗号/分号；
/// 不含路径点号时也拆 `/`，这样 “Windows/Linux” 不会要求整串连续出现。
/// 这不是汉语分词，只做轻量词项匹配，避免“硬件与软件资源”这类措辞差异误杀。
pub fn include_terms(phrase: &str) -> Vec<String> {
    let mut split = phrase.to_string();
    if !phrase.contains('.') {
        split = split.replace('/', "\u{0}");
    }
    split
        .split(|c: char| c.is_whitespace() || matches!(c, '、' | '，' | ',' | '；' | ';' | '\u{0}'))
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

/// `must_not_include` 里的 fixture 词/占位符：整条判据自我指涉，直接丢弃。
fn fixture_not_include(t: &str) -> bool {
    let t = t.trim();
    t == "REVIEW-FINAL"
        || t == "REVIEW-START"
        || t.contains("<...>")
        || t.contains("__FILL")
        || t.contains("{{")
        || t.contains("}}")
}

/// 兼容旧调用：没有任务输入时只做结构校验，不检查短语来源。
pub fn parse_rubric(text: &str) -> Result<Value, String> {
    parse_rubric_with_input(text, "")
}

/// 示例占位符的形态：尖括号模板、schema 里的空话，或模型直接把键名当值。
fn rubric_placeholder(s: &str) -> bool {
    let t = s.trim();
    t.is_empty()
        || (t.starts_with('<') && t.ends_with('>'))
        || (t.starts_with("__FILL") && t.ends_with("__"))
        || t.contains("{{")
        || t.contains("}}")
        || matches!(
            t,
            "这项检查什么"
                | "怎么核对"
                | "检查项"
                | "核对方式"
                | "criterion"
                | "check"
                | "..."
                | "…"
                | "这条判据检查的任务事实"
                | "怎样从答案核对"
                | "任务要求中必须出现的短语"
                | "禁止项说明"
                | "禁止哪些占位词"
                | "结构要求说明"
                | "至少几个要点、什么格式"
                | "需要 judge 判断的方面"
                | "读答案判断什么"
                | "至少几个要点，填数字"
        )
        || t.contains("这项检查什么")
        || t.contains("怎么核对")
}

/// 判据在讨论占位符/示例本身，而不是当前任务。只有同时出现“占位/示例”类元词
/// 和“禁止/检查/出现”类动作时才命中，避免误杀“解释模板方法”这类真实任务。
fn placeholder_meta(t: &str) -> bool {
    let t = t.trim();
    let meta = [
        "示例中的占位",
        "示例里的占位",
        "示例判据",
        "模板标记",
        "示例模板",
        "__FILL",
        "<...>",
    ];
    let intent = [
        "禁止", "不得", "不能", "不要", "检查", "出现", "使用", "包含", "提到", "填写",
    ];
    meta.iter().any(|m| t.contains(m)) && intent.iter().any(|i| t.contains(i))
}

/// M3：主观评价没有可核对标准，7B judge 每次判都可能不同。
fn subjective_reason(t: &str) -> Option<&'static str> {
    const WORDS: &[&str] = &[
        "简洁",
        "清晰",
        "美观",
        "通顺",
        "易懂",
        "可读性",
        "逻辑性",
        "完整性",
        "语气",
        "排版",
        "准确",
        "准确无误",
        "正确",
        "恰当",
        "专业",
        "规范",
        "流畅",
        "自然",
    ];
    WORDS.iter().find(|w| t.contains(**w)).copied()
}

/// 丢弃把占位符/示例当内容的判据；保留其余条目。返回清洗后的 JSON、丢弃说明、是否全丢。
fn strip_placeholder_meta_items(text: &str) -> (String, Vec<String>, bool) {
    let Ok(mut v) = serde_json::from_str::<Value>(json_slice(text)) else {
        return (text.to_string(), Vec::new(), false);
    };
    let arr = v
        .get("rubric")
        .or_else(|| v.get("items"))
        .and_then(|x| x.as_array())
        .or_else(|| v.as_array())
        .cloned();
    let Some(arr) = arr else {
        return (text.to_string(), Vec::new(), false);
    };
    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    for (i, item) in arr.iter().enumerate() {
        let id = item
            .get("id")
            .and_then(|x| x.as_str())
            .filter(|x| !x.trim().is_empty())
            .map(|x| x.to_string())
            .unwrap_or_else(|| format!("c{}", i + 1));
        let criterion = item.get("criterion").and_then(|x| x.as_str()).unwrap_or("");
        let check = item.get("check").and_then(|x| x.as_str()).unwrap_or("");
        // 空 criterion/check 是结构错误，必须交给正式解析报错，不能静默丢弃。
        let fixture_not = item
            .get("must_not_include")
            .map(|v| literal_list(v).iter().any(|b| fixture_not_include(b)))
            .unwrap_or(false);
        let junk = fixture_not
            || (!criterion.trim().is_empty()
                && (rubric_placeholder(criterion) || placeholder_meta(criterion)))
            || (!check.trim().is_empty() && (rubric_placeholder(check) || placeholder_meta(check)));
        if junk {
            dropped.push(format!("{id}（{criterion}）"));
        } else {
            kept.push(item.clone());
        }
    }
    if dropped.is_empty() {
        return (text.to_string(), dropped, false);
    }
    if kept.is_empty() {
        return (text.to_string(), dropped, true);
    }
    if v.get("rubric").and_then(|x| x.as_array()).is_some() {
        v["rubric"] = json!(kept);
    } else if v.get("items").and_then(|x| x.as_array()).is_some() {
        v["items"] = json!(kept);
    } else {
        v = Value::Array(kept);
    }
    (
        serde_json::to_string(&v).unwrap_or_else(|_| text.to_string()),
        dropped,
        false,
    )
}

pub fn parse_rubric_with_input(text: &str, input: &str) -> Result<Value, String> {
    let (text, dropped, all_dropped) = strip_placeholder_meta_items(text);
    if all_dropped {
        return Err(format!(
            "验收清单全是占位符/示例判据：{}。判据必须描述当前任务，不能讨论占位符或示例本身",
            dropped.join("；")
        ));
    }
    let v = parse_json_value(&text).map_err(|e| format!("验收清单不是合法 JSON: {e}"))?;
    let arr = v
        .get("rubric")
        .or_else(|| v.get("items"))
        .and_then(|x| x.as_array())
        .or_else(|| v.as_array())
        .ok_or("验收清单缺少 rubric 数组")?;
    if !(RUBRIC_MIN_ITEMS..=RUBRIC_MAX_ITEMS).contains(&arr.len()) {
        return Err(format!(
            "验收清单条数必须在 {RUBRIC_MIN_ITEMS}–{RUBRIC_MAX_ITEMS} 之间，实际 {}",
            arr.len()
        ));
    }
    // N24：明确的写文件任务由 judge 核对内核提供的文件内容；guide 猜出来的
    // must_include 不能拿来卡“最终答复”。这里把确定性键收走，统一转描述型。
    let file_task = crate::util::requests_file_write(input);
    let mut out = Vec::new();
    let mut hard_items = 0usize;
    let mut substring_only = true;
    let mut judge_only_ids = Vec::new();
    let mut advisory_ids = Vec::new();
    let mut advisory_warnings = Vec::new();
    for (i, item) in arr.iter().enumerate() {
        let criterion = item
            .get("criterion")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim();
        let check = item
            .get("check")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim();
        if criterion.is_empty() || check.is_empty() {
            return Err(format!("验收项 {} 缺少 criterion/check", i + 1));
        }
        if rubric_placeholder(criterion) || rubric_placeholder(check) {
            return Err(format!(
                "验收项 {} 的 criterion/check 是示例占位符或空话（criterion=『{criterion}』，check=『{check}』）：必须换成当前任务的具体判据",
                i + 1
            ));
        }
        let id = item
            .get("id")
            .and_then(|x| x.as_str())
            .filter(|x| !x.trim().is_empty())
            .map(|x| x.to_string())
            .unwrap_or_else(|| format!("c{}", i + 1));
        let weight = item.get("weight").and_then(|x| x.as_f64()).unwrap_or(1.0);
        let mut norm = serde_json::Map::new();
        norm.insert("id".into(), json!(id));
        norm.insert("criterion".into(), json!(criterion));
        norm.insert("check".into(), json!(check));
        norm.insert("weight".into(), json!(weight.clamp(0.1, 10.0)));
        // N29：写文件任务不按答案字面校验 guide 产出的硬键。文件任务的判据天然
        // 会写“行首 - ”这类符号（会被 2 字术语下限误杀），也可能与“必须有硬键”
        // 的普通任务规则冲突。这里一次性剥掉所有硬键，只留 criterion/check 给
        // judge 结合文件正文核对；空/占位 criterion 仍会被上面的检查拦住。
        if file_task {
            norm.insert("judge_only".into(), json!(true));
            norm.insert("file_task".into(), json!(true));
            judge_only_ids.push(id.clone());
            out.push(Value::Object(norm));
            continue;
        }
        // must_include：关键词/短语，禁整句，≤20 字。7B 有时写成数组（如 ["SYN","ACK"]），
        // 这里把 string / array 两种写法都收敛成列表，由 hardcheck 逐项核对。
        let mut must_include_values: Vec<String> =
            literal_list(item.get("must_include").unwrap_or(&Value::Null))
                .into_iter()
                .map(|x| {
                    x.trim_matches(|c| {
                        matches!(c, '『' | '』' | '「' | '」' | '`' | '"' | '“' | '”')
                    })
                    .trim()
                    .to_string()
                })
                .filter(|x| !x.is_empty())
                .collect();
        if must_include_values.is_empty() {
            if let Some(lit) = {
                // 只有在模型没写任何判据时才从 check 里提取『…』；否则 must_not_include 里的
                // 被禁词会被误当成 must_include，等于把禁词变成必选词。
                let has_explicit = item
                    .get("must_not_include")
                    .map(|v| !literal_list(v).is_empty())
                    .unwrap_or(false)
                    || item.get("min_points").is_some()
                    || item
                        .get("pattern")
                        .and_then(|x| x.as_str())
                        .map(|x| !x.trim().is_empty())
                        .unwrap_or(false)
                    || item
                        .get("regex")
                        .and_then(|x| x.as_str())
                        .map(|x| !x.trim().is_empty())
                        .unwrap_or(false)
                    || item.get("hard").is_some();
                if has_explicit {
                    None
                } else {
                    quoted_literal(check)
                }
            } {
                must_include_values.push(lit);
            }
        }
        for lit in &must_include_values {
            // N10：文件名/路径不是“答案原句”，12 字上限会把 workspace/notes.txt 误伤。
            // 路径/文件名放宽到 64 字；普通术语仍保持 12 字。
            let limit = if path_like(lit) {
                PATH_MUST_INCLUDE_MAX_CHARS
            } else {
                MUST_INCLUDE_MAX_CHARS
            };
            if lit.chars().count() > limit {
                let hint = if path_like(lit) {
                    "路径/文件名最多 64 字；如果真正要核对的是文件产出，建议改用 pattern/regex 或 criterion/check"
                } else {
                    "判据只能是短语，不能是答案原句"
                };
                return Err(format!(
                    "验收项 {} 的 must_include 超过 {limit} 字（{} 字）：{hint}",
                    i + 1,
                    lit.chars().count()
                ));
            }
            if sentence_like(lit) {
                return Err(format!(
                    "验收项 {} 的 must_include 是整句：判据只能是关键词/短语",
                    i + 1
                ));
            }
            if rubric_placeholder(lit) {
                return Err(format!(
                    "验收项 {} 的 must_include 是占位符『{lit}』：必须换成当前任务里必须出现的具体短语",
                    i + 1
                ));
            }
            let n_chars = lit.chars().count();
            if n_chars < 2 {
                return Err(format!(
                    "验收项 {} 的 must_include『{lit}』只有 {n_chars} 字：术语判据至少 2 字",
                    i + 1
                ));
            }
            if let Some(c) = include_connective(lit) {
                return Err(format!(
                    "验收项 {} 的 must_include『{lit}』含连接词『{c}』：这是概念性描述，不是术语。术语判据只写专有名词/固定写法；概念要求请写在 criterion/check 交给 judge",
                    i + 1
                ));
            }
            // L1：REVIEW-FINAL/REVIEW-START 是 mock 夹具的测试标记，7B 容易从示例里抄来。
            // 用户任务里明确出现时才允许；否则正常答案永远无法满足。
            // 只看报告里出现的两个 mock 标记；不要扩大到所有大写词，
            // 否则 “HTTP”“JSON” 这类任务内本来就该出现的术语会被误杀。
            let test_marker = matches!(lit.as_str(), "REVIEW-FINAL" | "REVIEW-START");
            // 集成测试的输入统一是 REVIEW_* 夹具令牌；生产任务不会长这样。
            let fixture_mode = input.contains("REVIEW_START")
                || input.contains("REVIEW-FINAL")
                || input.trim_start().starts_with("REVIEW_");
            if !input.trim().is_empty()
                && test_marker
                && !fixture_mode
                && !input.contains(lit.as_str())
            {
                return Err(format!(
                    "验收项 {} 的 must_include『{lit}』是示例/测试标记，且没有出现在用户任务输入中：判据必须来自当前任务",
                    i + 1
                ));
            }
        }
        let must_count = must_include_values.len();
        let match_mode = item
            .get("match")
            .and_then(|x| x.as_str())
            .map(|x| x.trim().to_lowercase())
            .filter(|x| !x.is_empty())
            .unwrap_or_else(|| {
                if must_count > 1 {
                    "min".into()
                } else {
                    "all".into()
                }
            });
        if must_count == 0 && item.get("match").is_some() {
            return Err(format!(
                "验收项 {} 写了 match，但 must_include 为空；match 只用于多项 must_include",
                i + 1
            ));
        }
        let include_min = if must_count == 0 {
            0
        } else {
            match match_mode.as_str() {
                "all" => must_count,
                "any" => 1,
                "min" => item
                    .get("min_matches")
                    .and_then(crate::util::json_u64)
                    .map(|n| n as usize)
                    .unwrap_or(must_count.div_ceil(2)),
                other => {
                    return Err(format!(
                        "验收项 {} 的 match 只能是 any/all/min，实际『{other}』",
                        i + 1
                    ))
                }
            }
            .clamp(1, must_count)
        };
        if must_count > 0 && item.get("min_matches").is_some() {
            let n = item
                .get("min_matches")
                .and_then(crate::util::json_u64)
                .unwrap_or(0) as usize;
            if n < 1 || n > must_count {
                return Err(format!(
                    "验收项 {} 的 min_matches 必须在 1–{must_count} 之间（must_include 共 {must_count} 项）",
                    i + 1
                ));
            }
        }
        if !must_include_values.is_empty() {
            let value = if must_include_values.len() == 1 {
                json!(must_include_values[0])
            } else {
                json!(must_include_values)
            };
            norm.insert("must_include".into(), value);
            norm.insert("match".into(), json!(match_mode));
            norm.insert("min_matches".into(), json!(include_min));
        }
        let banned = literal_list(item.get("must_not_include").unwrap_or(&Value::Null));
        if !banned.is_empty() {
            // N6：主观判据（如“术语准确”）不能用禁掉常用词的 must_not_include 来实现；
            // 这样会把“软件/硬件/计算机”这类答案必用词判死，失败原因还与判据无关。
            if let Some(word) = subjective_reason(criterion).or_else(|| subjective_reason(check)) {
                return Err(format!(
                    "验收项 {} 是主观判据（含『{word}』），不能用 must_not_include 实现；主观要求请只写 criterion/check 交给 judge，或改成可计数/可核对的 format",
                    i + 1
                ));
            }
            // 禁词已经出现在用户任务输入里：判据和任务自相矛盾，要求 guide 重写。
            let input_low = input.to_lowercase();
            if let Some(b) = banned
                .iter()
                .find(|b| !b.trim().is_empty() && input_low.contains(&b.to_lowercase()))
            {
                return Err(format!(
                    "验收项 {} 的 must_not_include『{b}』出现在用户任务输入中：禁词与任务矛盾，请改成当前任务真正应避免的内容",
                    i + 1
                ));
            }
            if let Some(b) = banned.iter().find(|b| rubric_placeholder(b)) {
                return Err(format!(
                    "验收项 {} 的 must_not_include 含占位符『{b}』：必须换成具体的禁词",
                    i + 1
                ));
            }
            if banned
                .iter()
                .any(|b| b.chars().count() > MUST_NOT_INCLUDE_MAX_CHARS)
            {
                return Err(format!(
                    "验收项 {} 的 must_not_include 超过 {MUST_NOT_INCLUDE_MAX_CHARS} 字",
                    i + 1
                ));
            }
            substring_only = false;
            norm.insert("must_not_include".into(), json!(banned));
        }
        if let Some(n) = item.get("min_points").and_then(|x| x.as_u64()) {
            if !(1..=12).contains(&n) {
                return Err(format!("验收项 {} 的 min_points 必须在 1–12 之间", i + 1));
            }
            substring_only = false;
            norm.insert("min_points".into(), json!(n));
        }
        if let Some(pat) = item
            .get("pattern")
            .and_then(|x| x.as_str())
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
        {
            if pat.chars().count() > PATTERN_MAX_CHARS {
                return Err(format!("验收项 {} 的 pattern 过长", i + 1));
            }
            substring_only = false;
            norm.insert("pattern".into(), json!(pat));
        }
        // 真正则判据：`regex` + 可选 `min_matches`（默认 1）。语法错误在 guide 阶段就拒绝，
        // 不要留到 hardcheck 才炸成 degraded。
        if let Some(pat) = item
            .get("regex")
            .and_then(|x| x.as_str())
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
        {
            if pat.chars().count() > REGEX_MAX_CHARS {
                return Err(format!("验收项 {} 的 regex 过长", i + 1));
            }
            regex::Regex::new(&pat)
                .map_err(|e| format!("验收项 {} 的 regex 不是合法正则：{e}", i + 1))?;
            substring_only = false;
            norm.insert("regex".into(), json!(pat));
            if let Some(n) = item.get("min_matches").and_then(|x| x.as_u64()) {
                if !(1..=50).contains(&n) {
                    return Err(format!("验收项 {} 的 min_matches 必须在 1–50 之间", i + 1));
                }
                norm.insert("min_matches".into(), json!(n));
            }
        }
        if item.get("hard").is_some() {
            norm.insert("hard".into(), item.get("hard").cloned().unwrap());
            substring_only = false;
        }
        // 只有描述、没有硬判据的条目不当废品：保留给 judge 逐条核对，
        // 由 `review.warn` 明示“这条没有确定性检查”，而不是整轮 degraded。
        // 写文件任务在循环开头已经走独立分支并 continue，这里只处理普通任务。
        let has_hard = norm.keys().any(|k| {
            matches!(
                k.as_str(),
                "must_include" | "must_not_include" | "min_points" | "pattern" | "regex" | "hard"
            )
        });
        if has_hard {
            hard_items += 1;
        } else if let Some(word) = subjective_reason(criterion).or_else(|| subjective_reason(check))
        {
            // M3：主观判据不进 judge，也不计入门禁；只留一条 review.warn。
            advisory_ids.push(id.clone());
            advisory_warnings.push(format!(
                "验收项 {id} 是主观判据（含『{word}』）：不计入门禁；需要时改成可计数/可核对的格式"
            ));
            norm.insert("advisory".into(), json!(true));
            norm.insert("advisory_reason".into(), json!(word));
        } else {
            judge_only_ids.push(id.clone());
            norm.insert("judge_only".into(), json!(true));
        }
        out.push(Value::Object(norm));
    }
    // 没有硬判据不直接判死：内核会带错误回注再要一次，仍拿不到才降级到 judge-only。
    // 这样 7B 不会因为“键名写错”整轮 degraded（rc.3 的 I1），同时错误信息足够具体。
    let mut warnings = advisory_warnings;
    for d in &dropped {
        warnings.push(format!(
            "已丢弃占位符/示例判据 {d}：判据要描述当前任务，不能讨论占位符或示例本身"
        ));
    }
    if file_task {
        warnings.push(
            "写文件任务：判据已转为 judge 核对文件内容与 tool.fs.write 成功记录；不按最终答复的字面措辞判定"
                .to_string(),
        );
    } else if hard_items == 0 {
        warnings.push(
            "整份清单没有硬判据：criterion/check 只是说明，判据必须写在 must_include / must_not_include / min_points / pattern / regex 上"
                .to_string(),
        );
    }
    if !judge_only_ids.is_empty() {
        warnings.push(format!(
            "{} 条描述型判据（{}）：由 judge 按 criterion 评估",
            judge_only_ids.len(),
            judge_only_ids.join(", ")
        ));
    } else {
        warnings.push("没有描述型判据：judge 不参与本轮，全部由确定性判据决定".to_string());
    }
    if substring_only && !file_task {
        warnings.push(
            "判据全是字面短语，建议再加一条 must_not_include / min_points / pattern 以覆盖格式与要点"
                .to_string(),
        );
    }
    Ok(json!({
        "rubric": out,
        "count": out.len(),
        "hard_items": hard_items,
        "judge_only": judge_only_ids,
        "advisory": advisory_ids,
        "file_task": file_task,
        "warn": warnings,
    }))
}

pub fn parse_verdict(text: &str, rubric: &Value, answer: &str) -> Result<Value, String> {
    let v = parse_json_value(text).map_err(|e| format!("审批结果不是合法 JSON: {e}"))?;
    let expected: Vec<String> = rubric
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| x.get("id").and_then(|y| y.as_str()).map(|s| s.to_string()))
        .collect();
    if expected.is_empty() {
        return Err("审批结果没有对应验收项".into());
    }
    let items = v
        .get("items")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let mut normalized = Vec::new();
    let mut failed = Vec::new();
    for id in &expected {
        let found = items
            .iter()
            .find(|x| x.get("id").and_then(|y| y.as_str()) == Some(id.as_str()));
        let Some(item) = found else {
            return Err(format!("审批结果缺少验收项 {id}"));
        };
        let pass = item.get("pass").and_then(|x| x.as_bool()).unwrap_or(false);
        let evidence = item
            .get("evidence")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        // N43：空 evidence 是“这一票无效”，不是“判据不合格”。两者必须分开报错，
        // 上层才会把空串当成可重试的无效票，而不是直接给整轮判 degraded。
        if evidence.is_empty() {
            return Err(format!(
                "审批项 {id} 的 evidence 为空：该票无效，必须重试并逐字复制答案原文，不是判据不合格"
            ));
        }
        // evidence 太短说明 judge 只是在占位（例如“总结”），属 judge 失败，交给上层重试。
        if !evidence_is_substantive(&evidence) {
            return Err(format!(
                "审批项 {id} 的 evidence 过短（须至少 6 个汉字或 3 个英文词）：{evidence:?}"
            ));
        }
        // evidence 必须能在答案里找到连续原文：抄提示词、写空话都不算证据。
        if !evidence_quotes(answer, &evidence) {
            return Err(format!(
                "审批项 {id} 的 evidence 不是答案原文（须为答案中连续 ≥6 字）：{evidence:?}"
            ));
        }
        if !pass {
            failed.push(Value::String(id.clone()));
        }
        let meta = rubric
            .as_array()
            .into_iter()
            .flatten()
            .find(|x| x.get("id").and_then(|y| y.as_str()) == Some(id.as_str()));
        let criterion = meta
            .and_then(|x| x.get("criterion"))
            .cloned()
            .unwrap_or(json!(""));
        let check = meta
            .and_then(|x| x.get("check"))
            .cloned()
            .unwrap_or(json!(""));
        normalized.push(json!({
            "id": id,
            "pass": pass,
            "evidence": evidence,
            "criterion": criterion,
            "check": check,
        }));
    }
    let summary = v
        .get("evidence")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim();
    if summary.is_empty() {
        return Err(
            "审批总结 evidence 为空：该票无效，必须重试并逐字复制答案原文，不是判据不合格"
                .to_string(),
        );
    }
    if !evidence_is_substantive(summary) {
        return Err(format!(
            "审批总结 evidence 过短（须至少 6 个汉字或 3 个英文词）：{summary:?}"
        ));
    }
    if !evidence_quotes(answer, summary) {
        return Err(format!(
            "审批总结 evidence 不是答案原文（须为答案中连续 ≥6 字）：{summary:?}"
        ));
    }
    let pass = failed.is_empty() && v.get("pass").and_then(|x| x.as_bool()).unwrap_or(false);
    Ok(json!({
        "pass": pass,
        "items": normalized,
        "failed": failed,
        "evidence": summary,
    }))
}

/// N43：空 evidence 是无效票，值得多给一次重试；过短/非原文仍按普通不合格处理。
pub fn verdict_error_is_empty_evidence(error: &str) -> bool {
    error.contains("evidence 为空")
}

/// 标准 JSON 解析失败时，修复字符串里的非法反斜杠转义与裸控制字符。
fn parse_json_value(text: &str) -> Result<Value, String> {
    let slice = json_slice(text);
    match serde_json::from_str::<Value>(slice) {
        Ok(v) => Ok(v),
        Err(first) => {
            let repaired = repair_json_text(slice);
            serde_json::from_str::<Value>(&repaired)
                .map_err(|e| format!("{e}（原始错误：{first}；已尝试修复非法转义/裸控制字符）"))
        }
    }
}

fn repair_json_text(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + 32);
    let mut i = 0usize;
    let mut in_string = false;
    while i < chars.len() {
        let c = chars[i];
        if !in_string {
            if c == '"' {
                in_string = true;
            }
            out.push(c);
            i += 1;
            continue;
        }
        match c {
            '"' => {
                in_string = false;
                out.push(c);
                i += 1;
            }
            '\\' => {
                let next = chars.get(i + 1).copied();
                let valid = match next {
                    Some('"') | Some('\\') | Some('/') | Some('b') | Some('f') | Some('n')
                    | Some('r') | Some('t') => true,
                    Some('u') => {
                        i + 5 < chars.len() && (1..=4).all(|k| chars[i + k].is_ascii_hexdigit())
                    }
                    _ => false,
                };
                if valid {
                    out.push('\\');
                    let n = chars[i + 1];
                    out.push(n);
                    i += 2;
                    if n == 'u' {
                        for k in 0..4 {
                            out.push(chars[i + k]);
                        }
                        i += 4;
                    }
                } else {
                    // 非法转义：补一个反斜杠，让 `\P` / `\w` 变成 `\P` / `\w`。
                    out.push('\\');
                    out.push('\\');
                    i += 1;
                }
            }
            '\n' => {
                out.push_str("\\n");
                i += 1;
            }
            '\r' => {
                out.push_str("\\r");
                i += 1;
            }
            '\t' => {
                out.push_str("\\t");
                i += 1;
            }
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_repairs_windows_backslash_escapes() {
        let rubric = json!([{"id": "c1", "criterion": "写入文件", "check": "路径与行数"}]);
        let answer = "已写入 C:\\Users\\tester\\workspace\\notes3.txt，共三行";
        let raw = r#"{"pass":true,"items":[{"id":"c1","pass":true,"evidence":"已写入 C:\Users\tester\workspace\notes3.txt"}],"evidence":"已写入 C:\Users\tester\workspace\notes3.txt"}"#;
        let v = parse_verdict(raw, &rubric, answer).expect("invalid escapes should repair");
        assert_eq!(v["pass"], true);
        assert_eq!(v["items"][0]["id"], "c1");
    }

    #[test]
    fn rubric_repairs_windows_backslash_escapes() {
        let raw = r#"{"rubric":[{"id":"c1","criterion":"写入 notes3.txt","check":"路径 C:\Users\tester\notes3.txt 存在"}]}"#;
        let v = parse_rubric(raw).expect("invalid escapes should repair");
        assert_eq!(v["count"], 1);
    }

    #[test]
    fn json_slice_accepts_top_level_array() {
        let raw = "```json\n[{\"id\":\"c1\"}]\n```";
        let v = parse_json_value(raw).expect("array");
        assert!(v.is_array());
    }

    #[test]
    fn verdict_accepts_short_chinese_evidence() {
        let rubric = json!([{"id": "c1", "criterion": "写入文件", "check": "三条要点"}]);
        let answer = "已完成写入，三条要点已写入文件。";
        let raw = r#"{"pass":true,"items":[{"id":"c1","pass":true,"evidence":"三条要点已写入文件"}],"evidence":"三条要点已写入文件"}"#;
        let v = parse_verdict(raw, &rubric, answer).expect("nine-char Chinese evidence is enough");
        assert_eq!(v["pass"], true);
    }

    #[test]
    fn verdict_accepts_three_word_english_evidence() {
        let rubric = json!([{"id": "c1", "criterion": "answer", "check": "value"}]);
        let answer = "The answer is four.";
        let raw = r#"{"pass":true,"items":[{"id":"c1","pass":true,"evidence":"The answer is four"}],"evidence":"The answer is four"}"#;
        let v = parse_verdict(raw, &rubric, answer).expect("three English words are enough");
        assert_eq!(v["pass"], true);
    }

    #[test]
    fn verdict_empty_evidence_is_invalid_vote_not_short_evidence() {
        let rubric = json!([{"id": "c3", "criterion": "写入文件", "check": "三条要点"}]);
        let answer = "已完成写入，三条要点已写入文件。";
        let raw = r#"{"pass":true,"items":[{"id":"c3","pass":true,"evidence":""}],"evidence":"三条要点已写入文件"}"#;
        let err = parse_verdict(raw, &rubric, answer).expect_err("empty evidence");
        assert!(err.contains("evidence 为空"), "{err}");
        assert!(!err.contains("evidence 过短"), "{err}");
        assert!(verdict_error_is_empty_evidence(&err));
    }

    #[test]
    fn verdict_short_evidence_stays_a_normal_failure() {
        let rubric = json!([{"id": "c3", "criterion": "写入文件", "check": "三条要点"}]);
        let answer = "已完成写入，三条要点已写入文件。";
        let raw = r#"{"pass":true,"items":[{"id":"c3","pass":true,"evidence":"总结"}],"evidence":"三条要点已写入文件"}"#;
        let err = parse_verdict(raw, &rubric, answer).expect_err("short evidence");
        assert!(err.contains("evidence 过短"), "{err}");
        assert!(!verdict_error_is_empty_evidence(&err));
    }

    #[test]
    fn verdict_empty_summary_is_invalid_vote() {
        let rubric = json!([{"id": "c1", "criterion": "answer", "check": "value"}]);
        let answer = "The answer is four.";
        let raw = r#"{"pass":true,"items":[{"id":"c1","pass":true,"evidence":"The answer is four"}],"evidence":""}"#;
        let err = parse_verdict(raw, &rubric, answer).expect_err("empty summary");
        assert!(err.contains("审批总结 evidence 为空"), "{err}");
        assert!(verdict_error_is_empty_evidence(&err));
    }
}
