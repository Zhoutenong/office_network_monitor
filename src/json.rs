//! 极简 JSON：只为本程序服务（读配置、出报告），不引入 serde。

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn parse(text: &str) -> Result<Json, String> {
        let bytes: Vec<char> = text.chars().collect();
        let mut parser = Parser { chars: bytes, pos: 0 };
        parser.skip_ws();
        let value = parser.value()?;
        parser.skip_ws();
        Ok(value)
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// 点号路径取值，如 "clash.proxy"
    pub fn path(&self, path: &str) -> Option<&Json> {
        let mut current = self;
        for part in path.split('.') {
            current = current.get(part)?;
        }
        Some(current)
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_arr(&self) -> Option<&Vec<Json>> {
        match self {
            Json::Arr(items) => Some(items),
            _ => None,
        }
    }

    pub fn string(&self, path: &str, fallback: &str) -> String {
        self.path(path)
            .and_then(|v| v.as_str())
            .unwrap_or(fallback)
            .to_string()
    }

    pub fn number(&self, path: &str, fallback: f64) -> f64 {
        self.path(path).and_then(|v| v.as_f64()).unwrap_or(fallback)
    }

    pub fn flag(&self, path: &str, fallback: bool) -> bool {
        self.path(path).and_then(|v| v.as_bool()).unwrap_or(fallback)
    }

    pub fn string_list(&self, path: &str) -> Vec<String> {
        self.path(path)
            .and_then(|v| v.as_arr())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn to_string_pretty(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0, true);
        out
    }



    fn write(&self, out: &mut String, depth: usize, pretty: bool) {
        let pad = |out: &mut String, level: usize| {
            if pretty {
                out.push('\n');
                for _ in 0..level {
                    out.push_str("  ");
                }
            }
        };
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Json::Num(value) => {
                if value.fract() == 0.0 && value.abs() < 1e15 {
                    out.push_str(&format!("{}", *value as i64));
                } else {
                    out.push_str(&format!("{}", value));
                }
            }
            Json::Str(text) => {
                out.push('"');
                out.push_str(&escape(text));
                out.push('"');
            }
            Json::Arr(items) => {
                if items.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    pad(out, depth + 1);
                    item.write(out, depth + 1, pretty);
                }
                pad(out, depth);
                out.push(']');
            }
            Json::Obj(items) => {
                if items.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push('{');
                for (index, (key, value)) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    pad(out, depth + 1);
                    out.push('"');
                    out.push_str(&escape(key));
                    out.push('"');
                    out.push(':');
                    if pretty {
                        out.push(' ');
                    }
                    value.write(out, depth + 1, pretty);
                }
                pad(out, depth);
                out.push('}');
            }
        }
    }
}

pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn skip_ws(&mut self) {
        while let Some(ch) = self.chars.get(self.pos) {
            if ch.is_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn value(&mut self) -> Result<Json, String> {
        self.skip_ws();
        match self.peek() {
            Some('{') => self.object(),
            Some('[') => self.array(),
            Some('"') => self.string().map(Json::Str),
            Some('t') => self.literal("true", Json::Bool(true)),
            Some('f') => self.literal("false", Json::Bool(false)),
            Some('n') => self.literal("null", Json::Null),
            Some(ch) if ch == '-' || ch.is_ascii_digit() => self.number(),
            other => Err(format!("意外的字符: {:?} (位置 {})", other, self.pos)),
        }
    }

    fn literal(&mut self, text: &str, value: Json) -> Result<Json, String> {
        for expected in text.chars() {
            if self.peek() != Some(expected) {
                return Err(format!("期望 {} (位置 {})", text, self.pos));
            }
            self.pos += 1;
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() || matches!(ch, '-' | '+' | '.' | 'e' | 'E') {
                self.pos += 1;
            } else {
                break;
            }
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        text.parse::<f64>()
            .map(Json::Num)
            .map_err(|_| format!("数字格式错误: {}", text))
    }

    fn string(&mut self) -> Result<String, String> {
        if self.peek() != Some('"') {
            return Err(format!("期望字符串 (位置 {})", self.pos));
        }
        self.pos += 1;
        let mut out = String::new();
        while let Some(ch) = self.peek() {
            self.pos += 1;
            match ch {
                '"' => return Ok(out),
                '\\' => {
                    let escape = self.peek().ok_or("字符串未结束")?;
                    self.pos += 1;
                    match escape {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            let mut code = 0u32;
                            for _ in 0..4 {
                                let digit = self.peek().ok_or("\\u 未结束")?;
                                self.pos += 1;
                                code = code * 16 + digit.to_digit(16).ok_or("\\u 非法")?;
                            }
                            out.push(char::from_u32(code).unwrap_or('?'));
                        }
                        other => return Err(format!("未知转义: \\{}", other)),
                    }
                }
                c => out.push(c),
            }
        }
        Err("字符串未结束".to_string())
    }

    fn array(&mut self) -> Result<Json, String> {
        self.pos += 1; // '['
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            if self.peek() == Some(']') {
                self.pos += 1;
                return Ok(Json::Arr(items));
            }
            items.push(self.value()?);
            self.skip_ws();
            match self.peek() {
                Some(',') => self.pos += 1,
                Some(']') => {
                    self.pos += 1;
                    return Ok(Json::Arr(items));
                }
                other => return Err(format!("数组分隔符错误: {:?}", other)),
            }
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.pos += 1; // '{'
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            if self.peek() == Some('}') {
                self.pos += 1;
                return Ok(Json::Obj(items));
            }
            let key = self.string()?;
            self.skip_ws();
            if self.peek() != Some(':') {
                return Err(format!("缺少冒号 (位置 {})", self.pos));
            }
            self.pos += 1;
            let value = self.value()?;
            items.push((key, value));
            self.skip_ws();
            match self.peek() {
                Some(',') => self.pos += 1,
                Some('}') => {
                    self.pos += 1;
                    return Ok(Json::Obj(items));
                }
                other => return Err(format!("对象分隔符错误: {:?}", other)),
            }
        }
    }
}
