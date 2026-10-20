//! Lean 4.34.1 lexical boundaries; shared conformance corpus with zk_registry.
//! Parser.Basic, Parser.StrInterpolation and Init.Meta.Defs are the authority.

fn letter_like(c: char) -> bool {
    matches!(c as u32, 0x3b1..=0x3c9) && c != 'λ'
        || matches!(c as u32, 0x391..=0x3a9) && c != 'Π' && c != 'Σ'
        || matches!(c as u32, 0x3ca..=0x3fb | 0x1f00..=0x1ffe | 0x2100..=0x214f | 0x1d49c..=0x1d59f | 0x100..=0x17f)
        || matches!(c as u32, 0xc0..=0xff) && c != '×' && c != '÷'
}
fn id_first(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || letter_like(c)
}
fn id_rest(c: char) -> bool {
    id_first(c)
        || c.is_ascii_digit()
        || matches!(c, '\'' | '!' | '?')
        || matches!(c as u32, 0x2080..=0x2089 | 0x2090..=0x209c | 0x1d62..=0x1d6a | 0x2c7c)
}

struct Lexer {
    src: Vec<char>,
    out: Vec<char>,
}
impl Lexer {
    fn at(&self, i: usize, s: &str) -> bool {
        self.src[i..]
            .iter()
            .copied()
            .zip(s.chars())
            .all(|(a, b)| a == b)
            && self.src.len() - i >= s.chars().count()
    }
    fn escape(&self, mut i: usize, string: bool, interpolated: bool) -> Result<usize, ()> {
        let c = *self.src.get(i).ok_or(())?;
        if matches!(c, '\\' | '"' | '\'' | 'r' | 'n' | 't') || interpolated && c == '{' {
            return Ok(i + 1);
        }
        if c == 'x' || c == 'u' {
            let end = i + if c == 'x' { 3 } else { 5 };
            if end <= self.src.len() && self.src[i + 1..end].iter().all(char::is_ascii_hexdigit) {
                return Ok(end);
            }
        }
        if string && c == '\n' {
            i += 1;
            while self
                .src
                .get(i)
                .is_some_and(|c| matches!(c, ' ' | '\t' | '\r'))
            {
                i += 1;
            }
            if self.src.get(i) != Some(&'\n') {
                return Ok(i);
            }
        }
        Err(())
    }
    fn identifier(&mut self, mut i: usize) -> Result<usize, ()> {
        let start = i;
        loop {
            if self.src[i] == '«' {
                i += 1;
                while self.src.get(i).is_some_and(|c| *c != '»') {
                    i += 1;
                }
                if i == self.src.len() {
                    return Err(());
                }
                i += 1;
            } else {
                i += 1;
                while self.src.get(i).is_some_and(|c| id_rest(*c)) {
                    i += 1;
                }
            }
            if self.src.get(i) == Some(&'.')
                && self
                    .src
                    .get(i + 1)
                    .is_some_and(|c| id_first(*c) || *c == '«')
            {
                i += 1;
            } else {
                self.out[start..i].copy_from_slice(&self.src[start..i]);
                return Ok(i);
            }
        }
    }
    fn interpolation_context(&self, mut i: usize) -> Result<bool, ()> {
        while i > 0 && matches!(self.out[i - 1], ' ' | '\t' | '\r' | '\n') {
            i -= 1;
        }
        let end = i;
        while i > 0 && id_rest(self.out[i - 1]) {
            i -= 1;
        }
        let prefix = self.out[i..end].iter().collect::<String>();
        if prefix.ends_with('!') {
            return if matches!(prefix.as_str(), "s!" | "m!" | "f!" | "println!") {
                Ok(true)
            } else {
                Err(())
            };
        }
        Ok(matches!(
            prefix.as_str(),
            "dbg_trace"
                | "throwError"
                | "throwNamedError"
                | "throwNamedErrorAt"
                | "logNamedError"
                | "logNamedErrorAt"
                | "logNamedWarning"
                | "logNamedWarningAt"
        ))
    }
    fn string(&mut self, start: usize, depth: usize) -> Result<usize, ()> {
        let interpolated = self.interpolation_context(start)?;
        let mut i = start + 1;
        while i < self.src.len() {
            match self.src[i] {
                '"' => return Ok(i + 1),
                '\\' => i = self.escape(i + 1, true, interpolated)?,
                '{' if interpolated => i = self.code(i + 1, true, depth + 1)?,
                // Contextual interpolation macros need argument parsing. A raw
                // string spells literal braces without this ambiguity.
                '{' => return Err(()),
                _ => i += 1,
            }
        }
        Err(())
    }
    fn code(&mut self, mut i: usize, close: bool, depth: usize) -> Result<usize, ()> {
        if depth > 128 {
            return Err(());
        }
        while i < self.src.len() {
            let c = self.src[i];
            if close && c == '}' {
                return Ok(i + 1);
            }
            if self.at(i, "--") {
                while i < self.src.len() && self.src[i] != '\n' {
                    i += 1;
                }
            } else if self.at(i, "/-") {
                let mut nesting = 1;
                i += 2;
                while i < self.src.len() && nesting > 0 {
                    if self.at(i, "/-") {
                        nesting += 1;
                        i += 2;
                    } else if self.at(i, "-/") {
                        nesting -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                if nesting != 0 {
                    return Err(());
                }
            } else {
                let mut raw_quote = i + 1;
                if c == 'r' {
                    while self.src.get(raw_quote) == Some(&'#') {
                        raw_quote += 1;
                    }
                }
                if c == 'r' && self.src.get(raw_quote) == Some(&'"') {
                    let endmark = format!("\"{}", "#".repeat(raw_quote - i - 1));
                    i = raw_quote + 1;
                    while i < self.src.len() && !self.at(i, &endmark) {
                        i += 1;
                    }
                    if i == self.src.len() {
                        return Err(());
                    }
                    i += endmark.len();
                } else if c == '«' || id_first(c) {
                    i = self.identifier(i)?;
                } else if c == '»' {
                    return Err(());
                } else if c == '"' {
                    i = self.string(i, depth)?;
                } else if c == '\'' {
                    i += 1;
                    let next = *self.src.get(i).ok_or(())?;
                    i = if next == '\\' {
                        self.escape(i + 1, false, false)?
                    } else {
                        i + 1
                    };
                    if self.src.get(i) != Some(&'\'') {
                        return Err(());
                    }
                    i += 1;
                } else if close && c == '{' {
                    self.out[i] = c;
                    i = self.code(i + 1, true, depth + 1)?;
                    self.out[i - 1] = '}';
                } else {
                    self.out[i] = c;
                    i += 1;
                }
            }
        }
        if close {
            Err(())
        } else {
            Ok(i)
        }
    }
}

pub(super) fn blank_non_code(text: &str) -> Result<String, ()> {
    let mut lexer = Lexer {
        src: text.chars().collect(),
        out: text
            .chars()
            .map(|c| if c == '\n' { '\n' } else { ' ' })
            .collect(),
    };
    lexer.code(0, false, 0)?;
    Ok(lexer.out.into_iter().collect())
}
